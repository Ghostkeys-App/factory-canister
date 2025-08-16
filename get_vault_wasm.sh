#!/bin/sh

# This script fetches the latest vault canister wasm file from the GitHub repository

gh api -H "Accept: application/vnd.github+json" -H "X-GitHub-Api-Version: 2022-11-28" /repos/Ghostkeys-App/vault-canister/actions/artifacts/$(gh api -H "Accept: application/vnd.github+json" -H "X-GitHub-Api-Version: 2022-11-28" /repos/Ghostkeys-App/vault-canister/actions/artifacts | jq .artifacts[0].id)/zip > ./vault_canister.zip

unzip -o vault_canister.zip -d ./target/wasm32-unknown-unknown/release/
rm vault_canister.zip