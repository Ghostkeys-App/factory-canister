use candid::{CandidType, Principal};
use ic_cdk::{
    api::{canister_self, msg_caller},
    management_canister::{
        create_canister_with_extra_cycles, install_code, CanisterSettings, CreateCanisterArgs,
        CreateCanisterResult, InstallCodeArgs,
    },
    query,
    storage::{stable_restore, stable_save},
    update,
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

// Don't forget to call dfx canister deposit-cycles factory-canister-backend <amount>.
const CREATE_CYCLES: u128 = 30_000_000_000;

#[derive(Clone, Default, CandidType, Deserialize, Serialize)]
struct FactoryState {
    owner_to_vault: BTreeMap<Principal, Principal>,
    last_create_nanos: BTreeMap<Principal, u64>,
    min_create_interval_ns: u64,
}

thread_local! {
    static STATE: std::cell::RefCell<FactoryState> = std::cell::RefCell::new(FactoryState {
        min_create_interval_ns: 5_000_000_000,
        ..Default::default()
    });
}

#[query]
fn lookup_vault(owner: Principal) -> Option<Principal> {
    STATE.with(|s| s.borrow().owner_to_vault.get(&owner).cloned())
}

#[update]
async fn get_or_create_vault() -> Principal {
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

    let wasm_bytes: Vec<u8> = include_bytes!(env!("VAULT_WASM_PATH")).to_vec(); // TODO: check if we can access binary of git repo

    let install = InstallCodeArgs {
        mode: ic_cdk::management_canister::CanisterInstallMode::Install,
        canister_id: create_res.canister_id,
        wasm_module: wasm_bytes,
        arg: candid::encode_one(user).unwrap(),
    };

    let _: () = install_code(&install).await.expect("install_code failed");

    STATE.with(|s| s.borrow_mut().owner_to_vault.insert(user, vault_id));

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

#[ic_cdk::pre_upgrade]
fn pre_upgrade() {
    STATE.with(|s| stable_save((s.borrow().clone(),)).unwrap());
}

#[ic_cdk::post_upgrade]
fn post_upgrade() {
    let (st,): (FactoryState,) = stable_restore().unwrap_or_default();
    STATE.with(|s| *s.borrow_mut() = st);
}
