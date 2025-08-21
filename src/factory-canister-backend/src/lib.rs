use candid::{CandidType, Principal};
use ic_cdk::{
    api::{canister_self, msg_caller},
    management_canister::{
        create_canister_with_extra_cycles, deposit_cycles, install_code, CanisterSettings, CreateCanisterArgs, CreateCanisterResult, DepositCyclesArgs, InstallCodeArgs
    },
    query,
    storage::{stable_restore, stable_save},
    update,
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

// Don't forget to call dfx canister deposit-cycles factory-canister-backend <amount>.
const CREATE_CYCLES: u128 = 500_000_000_000;
const TOP_UP_CYCLES: u128 = 100_000_000_000;

#[derive(Clone, Default, CandidType, Deserialize, Serialize)]
struct FactoryState {
    owner_to_vault: BTreeMap<Principal, Principal>,
    shared_vaults_to_users: BTreeMap<Principal, Vec<Principal>>,
    last_create_nanos: BTreeMap<Principal, u64>,
    min_create_interval_ns: u64,
}

thread_local! {
    static STATE: std::cell::RefCell<FactoryState> = std::cell::RefCell::new(FactoryState {
        min_create_interval_ns: 5_000_000_000,
        ..Default::default()
    });
}

fn log(msg: String) {
    ic_cdk::println!("{}", msg);
}

#[query]
fn lookup_vault(owner: Principal) -> Option<Principal> {
    STATE.with(|s| s.borrow().owner_to_vault.get(&owner).cloned())
}

#[query]
fn lookup_shared_vault(owner: Principal) -> Option<Principal> {
    STATE.with(|s| s.borrow().shared_vaults_to_users.iter().filter_map(|(vault, users)| {
        if users.contains(&owner) {
            Some(vault.clone())
        } else {
            None
        }
    }).next())
}

#[update]
async fn get_or_create_vault() -> Principal {
    log("get_or_create_vault called".to_string());
    let user = msg_caller();

    if let Some(cid) = lookup_vault(user) {
        return cid;
    }

    throttle(&user);

    let settings = CanisterSettings {
        controllers: Some(vec![user, canister_self()]),
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

    let wasm_bytes: Vec<u8> = include_bytes!("../../../target/wasm32-unknown-unknown/release/vault_canister_backend.wasm").to_vec(); // TODO: check if we can access binary of git repo

    let install = InstallCodeArgs {
        mode: ic_cdk::management_canister::CanisterInstallMode::Install,
        canister_id: create_res.canister_id,
        wasm_module: wasm_bytes,
        arg: candid::encode_args((user.to_text(), canister_self().to_text())).unwrap(),
    };

    let _: () = install_code(&install).await.expect("install_code failed");

    STATE.with(|s| s.borrow_mut().owner_to_vault.insert(user, vault_id));

    log(format!("Created vault for user: {}", user.to_text()));

    vault_id
}

fn associate_user_to_shared_vault(vault_id: Principal, user: Principal) {
    STATE.with(|s| {
        let mut st = s.borrow_mut();
        st.shared_vaults_to_users.entry(vault_id).or_default().push(user);
    });
}

#[update]
async fn get_or_create_shared_vault() -> Principal {
    log("get_or_create_shared_vault called".to_string());
    let user = msg_caller();

    if let Some(cid) = lookup_shared_vault(user) {
        return cid;
    }

    if !STATE.with(|s| s.borrow().shared_vaults_to_users.is_empty())
    {
        // If there are existing shared vaults, we can use one of them.
        let vault_id = STATE.with(|s| s.borrow().shared_vaults_to_users.keys().next().cloned()).expect("No shared vaults available");
        associate_user_to_shared_vault(vault_id, user);
        return vault_id
    }

    throttle(&user);

    let settings = CanisterSettings {
        controllers: Some(vec![user, canister_self()]),
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

    let wasm_bytes: Vec<u8> = include_bytes!("../../../target/wasm32-unknown-unknown/release/shared_vault_canister_backend.wasm").to_vec();

    let install = InstallCodeArgs {
        mode: ic_cdk::management_canister::CanisterInstallMode::Install,
        canister_id: create_res.canister_id,
        wasm_module: wasm_bytes,
        arg: candid::encode_args((user.to_text(), canister_self().to_text())).unwrap(),
    };

    let _: () = install_code(&install).await.expect("install_code failed");

    STATE.with(|s| s.borrow_mut().owner_to_vault.insert(user, vault_id));

    log(format!("Created shared vault for user: {}", user.to_text()));

    vault_id
}

fn throttle(user: &Principal) {
    let now = ic_cdk::api::time() as u64;
    STATE.with(|s| {
        let mut st = s.borrow_mut();
        if let Some(last) = st.last_create_nanos.get(user).cloned() {
            if now.saturating_sub(last) < st.min_create_interval_ns {
                ic_cdk::trap("rate limited");
            }
        }
        st.last_create_nanos.insert(*user, now);
    });
}

#[update]
pub async fn top_up() -> Result<(), String> {
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

ic_cdk::export_candid!();