use candid::{encode_one, CandidType, Principal};
use ic_cdk::{api::{call::call_with_payment128, canister_self, msg_caller}, management_canister::{CanisterSettings, CreateCanisterArgs, CreateCanisterResult, InstallChunkedCodeArgs}, query, storage::{stable_restore, stable_save}, update};
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
        wasm_memory_threshold: None
    };

    let arg = (CreateCanisterArgs {
        settings: Some(settings),
    },);

    let (create_res,): (CreateCanisterResult,) = call_with_payment128(
        Principal::management_canister(),
        "create_canister",
        arg,
        CREATE_CYCLES,
    )
    .await
    .expect("create_canister failed (insufficient cycles?)");

    let vault_id = create_res.canister_id;

    let init_arg: Vec<u8> = encode_one(user).expect("encode init arg");
    let wasm_bytes: Vec<u8> = include_bytes!(env!("VAULT_WASM_PATH")).to_vec();

    let install = InstallChunkedCodeArgs {
        mode: ic_cdk::management_canister::CanisterInstallMode::Install,
        target_canister: vault_id,
        wasm_module_hash: wasm_bytes,
        arg: init_arg,
        store_canister: None,
        chunk_hashes_list: Default::default()
    };

    let _: () = ic_cdk::api::call::call(
        Principal::management_canister(),
        "install_code",
        (install,),
    )
    .await
    .expect("install_code failed");

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
