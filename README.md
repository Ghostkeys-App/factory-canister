# `factory-canister`

This canister manages the creation of new vault canisters, and acts as a gateway to existing vault canisters.

To learn more before you start working with `factory-canister`, see the following documentation available online:

- [Quick Start](https://internetcomputer.org/docs/current/developer-docs/setup/deploy-locally)
- [SDK Developer Tools](https://internetcomputer.org/docs/current/developer-docs/setup/install)
- [Rust Canister Development Guide](https://internetcomputer.org/docs/current/developer-docs/backend/rust/)
- [ic-cdk](https://docs.rs/ic-cdk)
- [ic-cdk-macros](https://docs.rs/ic-cdk-macros)
- [Candid Introduction](https://internetcomputer.org/docs/current/developer-docs/backend/candid/)

## Quick Start Guide


### Pre-requisites

First install the build pre-requisites:
If you want to have control over spawned canisters add init_arg to dfx:

`"init_arg": "(opt record { admin = opt principal \"\" })"`

### Running the project locally

If you want to test the factory-canister locally, use the following commands

```bash
# Starts the replica, running in the background
dfx start --background
dfx deploy
```

Once the job completes, your application will be available at `http://localhost:4943?canisterId={asset_canister_id}`.