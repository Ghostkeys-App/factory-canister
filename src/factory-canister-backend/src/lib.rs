use candid::{CandidType, Principal};
use ic_cdk::{
    api::{canister_self, msg_caller},
    call::Call,
    inspect_message,
    management_canister::{
        create_canister_with_extra_cycles, deposit_cycles, install_code, CanisterSettings,
        CreateCanisterArgs, CreateCanisterResult, DepositCyclesArgs, InstallCodeArgs,
    },
    query,
    storage::{stable_restore, stable_save},
    update,
};
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, time::Duration};

// Don't forget to call dfx canister deposit-cycles factory-canister-backend <amount>.
const CREATE_CYCLES: u128 = 500_000_000_000;
const TOP_UP_CYCLES: u128 = 100_000_000_000;

// Type for Default controllers
type SharedCanisterControllers = Vec<Principal>;

#[derive(Clone, Default, CandidType, Deserialize, Serialize)]
struct FactoryState {
    owner_to_vault: BTreeMap<Principal, Principal>,
    users_to_shared_vault: BTreeMap<Principal, Principal>,
    known_shared_vaults: Vec<Principal>,
    free_shared_vault: Option<Principal>,
    last_create_nanos: BTreeMap<Principal, u64>,
    min_create_interval_ns: u64,
}

#[derive(CandidType, Deserialize)]
struct InitArgs {
    admin: Option<candid::Principal>,
}

thread_local! {
    static STATE: std::cell::RefCell<FactoryState> = std::cell::RefCell::new(FactoryState {
        min_create_interval_ns: 5_000_000_000,
        ..Default::default()
    });
    static DEFAULT_CONTROLLERS: std::cell::RefCell<SharedCanisterControllers> = std::cell::RefCell::new(vec![canister_self()]);
}

fn log(msg: String) {
    ic_cdk::println!("{}", msg);
}

#[inspect_message]
fn inspect_message() {
    let always_accept: Vec<String> = vec![
        "get_shared_vault".to_string(),
        "notify_canister_at_capacity".to_string(),
        "register_shared_vault_user".to_string(),
        "top_up".to_string()
    ];
    DEFAULT_CONTROLLERS.with(|dc| {
        let controllers_ref = dc.borrow();
        if controllers_ref.contains(&ic_cdk::api::msg_caller())
            || always_accept.contains(&ic_cdk::api::msg_method_name())
        {
            ic_cdk::api::accept_message();
        } else {
            ic_cdk::println!("Unauthorized caller: {}", ic_cdk::api::msg_caller());
            ic_cdk::trap(format!(
                "Unauthorized caller: {}",
                ic_cdk::api::msg_caller()
            ));
        }
    });
}

async fn init_create_shared_vault() {
    log("Creating a new shared vault".to_string());

    // By default share vault canister can be controlled by FactoryCanister, Deployer of Factory canister and optionaly provided Principal through init args
    let controllers: SharedCanisterControllers =
        DEFAULT_CONTROLLERS.with(|dc| SharedCanisterControllers::from(dc.borrow().clone()));

    let admin_principal = controllers
        .get(2)
        .cloned()
        .unwrap_or_else(|| canister_self());

    let settings = CanisterSettings {
        controllers: Some(controllers),
        compute_allocation: None,
        memory_allocation: None,
        freezing_threshold: None,
        reserved_cycles_limit: None,
        log_visibility: None,
        wasm_memory_limit: None,
        wasm_memory_threshold: None,
    };

    let arg = CreateCanisterArgs {
        settings: Some(settings),
    };

    let create_res: CreateCanisterResult = create_canister_with_extra_cycles(&arg, CREATE_CYCLES)
        .await
        .expect("create_canister_with_extra_cycles failed | insufficient funds?");

    let vault_id = create_res.canister_id;

    let wasm_bytes: Vec<u8> = include_bytes!(
        "../../../target/wasm32-unknown-unknown/release/shared_vault_canister_backend.wasm"
    )
    .to_vec();

    let install = InstallCodeArgs {
        mode: ic_cdk::management_canister::CanisterInstallMode::Install,
        canister_id: create_res.canister_id,
        wasm_module: wasm_bytes,
        arg: Vec::default(),
    };

    let _: () = install_code(&install).await.expect("install_code failed");
    let _ = Call::unbounded_wait(vault_id, "shared_canister_init")
        .with_args(&(admin_principal, canister_self()));

    STATE.with(|s| {
        let mut state = s.borrow_mut();
        state.free_shared_vault = Some(vault_id);
        state.known_shared_vaults.push(vault_id);
    });

    log(format!(
        "Created shared vault with ID: {}",
        vault_id.to_text()
    ));
}

#[ic_cdk::init]
fn init(args: Option<InitArgs>) {
    let admin = args.and_then(|a| a.admin);
    DEFAULT_CONTROLLERS.with(|dc| {
        dc.borrow_mut().push(msg_caller());
        if admin.is_some() {
            dc.borrow_mut().push(admin.unwrap());
        }
    });
    log("Factory canister initialized".to_string());
    ic_cdk_timers::set_timer(Duration::from_secs(1), || {
        ic_cdk::futures::spawn(async {
            log("Running init_create_shared_vault".to_string());
            let _ = init_create_shared_vault().await;
            log("Completed init_create_shared_vault".to_string());
        });
    });
    log("Set timer to create shared vault".to_string());
}

#[update]
async fn notify_canister_at_capacity() {
    let vault = msg_caller();
    // If the free shared vault is the one notifying, or if there is no free shared vault, create a new one
    STATE.with(|s| {
        let mut st = s.borrow_mut();
        if st.free_shared_vault.is_none() || st.free_shared_vault == Some(vault) {
            st.free_shared_vault = None;
            // Create a new shared vault
            ic_cdk::futures::spawn(async {
                log("Creating a new shared vault due to capacity notification".to_string());
                let _ = init_create_shared_vault().await;
                log("Completed creating new shared vault".to_string());
            });
        } else {
            log(
                "notify_canister_at_capacity called by non-free shared vault, ignoring".to_string(),
            );
            ic_cdk::trap("Only the free shared vault can notify at capacity");
        }
    });
}

#[query]
fn lookup_vault(owner: Principal) -> Option<Principal> {
    STATE.with(|s| s.borrow().owner_to_vault.get(&owner).cloned())
}

#[query]
fn get_shared_vault() -> Principal {
    let owner = msg_caller();
    // Check if the user has a shared vault and return it, otherwise return the current free shared vault
    STATE.with(|s| {
        if let Some(vault) = s.borrow().users_to_shared_vault.get(&owner) {
            return vault.clone();
        } else {
            return s.borrow().free_shared_vault.clone().unwrap_or_else(|| {
                log("No shared vault available, this should never happen.".to_string());
                Principal::anonymous()
            });
        }
    })
}

#[update]
fn register_shared_vault_user(user: Principal) -> Result<(), String> {
    let vault = msg_caller();
    if vault == Principal::anonymous() {
        return Err("No shared vault available".to_string());
    }

    STATE.with(|s| {
        let mut st = s.borrow_mut();
        if st.users_to_shared_vault.contains_key(&user) {
            return Err("User already registered for shared vault".to_string());
        }
        if !st.known_shared_vaults.contains(&vault) {
            return Err("Caller is not a known shared vault".to_string());
        }
        st.users_to_shared_vault.insert(user, vault);
        Ok(())
    })
}

#[update]
fn delete_user() -> Result<(), String> {
    let user_id = msg_caller();
    STATE.with(|s| {
        let mut st = s.borrow_mut();

        if !st.users_to_shared_vault.contains_key(&user_id) {
            ic_cdk::trap("User not recognised".to_string());
        }
        let vault = st.users_to_shared_vault.get(&user_id).unwrap().clone();
        st.users_to_shared_vault.remove(&user_id);

        Call::unbounded_wait(vault, "purge_user_data");
        Ok(())
    })
}

#[query]
fn get_controlled_canister_ids() -> Vec<Principal> {
    STATE.with(|s| s.borrow().known_shared_vaults.clone())
}

#[update]
pub async fn top_up() -> Result<(), String> {
    let vault = msg_caller();
    // If the shared vault is the one notifying
    STATE.with(|s| {
        let st = s.borrow();
        if !st.known_shared_vaults.contains(&vault) {
            log(
                "notify_canister_at_capacity called by non-free shared vault, ignoring".to_string(),
            );
            ic_cdk::trap("Only the free shared vault can notify at capacity");
        }
    });
    let can_record = DepositCyclesArgs {
        canister_id: msg_caller(),
    };
    match deposit_cycles(&can_record, TOP_UP_CYCLES).await {
        Ok(_) => Ok(()),
        Err(e) => Err(format!("Failed to top up cycles: {}", e)),
    }
}

#[ic_cdk::pre_upgrade]
fn pre_upgrade() {
    STATE.with(|s| stable_save((s.borrow().clone(),)).unwrap());
}

#[ic_cdk::post_upgrade]
fn post_upgrade() {
    let (st,): (FactoryState,) = stable_restore().unwrap_or_default();
    STATE.with(|s| *s.borrow_mut() = st);
}

// This is API call designed for Premium Users to have their own canisters
// #[update]
// async fn get_or_create_vault() -> Principal {
//     log("get_or_create_vault called".to_string());
//     let user = msg_caller();

//     if let Some(cid) = lookup_vault(user) {
//         return cid;
//     }

//     throttle(&user);

//     let settings = CanisterSettings {
//         controllers: Some(vec![user, canister_self()]),
//         compute_allocation: None,
//         memory_allocation: None,
//         freezing_threshold: None,
//         reserved_cycles_limit: None,
//         log_visibility: None,
//         wasm_memory_limit: None,
//         wasm_memory_threshold: None,
//     };

//     let arg = CreateCanisterArgs {
//         settings: Some(settings),
//     };

//     let create_res: CreateCanisterResult = create_canister_with_extra_cycles(&arg, CREATE_CYCLES)
//         .await
//         .expect("create_canister_with_extra_cycles failed | insufficient funds?");

//     let vault_id = create_res.canister_id;

//     let wasm_bytes: Vec<u8> = include_bytes!("../../../target/wasm32-unknown-unknown/release/vault_canister_backend.wasm").to_vec(); // TODO: check if we can access binary of git repo

//     let install = InstallCodeArgs {
//         mode: ic_cdk::management_canister::CanisterInstallMode::Install,
//         canister_id: create_res.canister_id,
//         wasm_module: wasm_bytes,
//         arg: Vec::default(),
//     };

//     let _: () = install_code(&install).await.expect("install_code failed");

//     let _ = Call::unbounded_wait(
//         vault_id,
//         "shared_canister_init",
//     ).with_args(&(user, canister_self())).await.expect("shared_canister_init failed");

//     STATE.with(|s| s.borrow_mut().owner_to_vault.insert(user, vault_id));

//     log(format!("Created vault for user: {}", user.to_text()));

//     vault_id
// }

// For premium users only
// fn throttle(user: &Principal) {
//     let now = ic_cdk::api::time() as u64;
//     STATE.with(|s| {
//         let mut st = s.borrow_mut();
//         if let Some(last) = st.last_create_nanos.get(user).cloned() {
//             if now.saturating_sub(last) < st.min_create_interval_ns {
//                 ic_cdk::trap("rate limited");
//             }
//         }
//         st.last_create_nanos.insert(*user, now);
//     });
// }

ic_cdk::export_candid!();
