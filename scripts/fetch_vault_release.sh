#!/usr/bin/env bash
set -euo pipefail

# Allow override: export VAULT_RELEASE_TAG=my-tag before running dfx build
TAG="${VAULT_RELEASE_TAG:-shared-canister-dev-v0.6.3}"
BASE="https://github.com/Ghostkeys-App/vault-canister/releases/download/${TAG}"

WASM_DIR="target/wasm32-unknown-unknown/release"
VAULT_WASM="${WASM_DIR}/vault_canister_backend.wasm"
SHARED_WASM="${WASM_DIR}/shared_vault_canister_backend.wasm"

VAULT_DID_PATH="src/vault-canister-backend/vault-canister-backend.did"
SHARED_DID_PATH="src/shared-vault-canister-backend/shared-vault-canister-backend.did"

mkdir -p "${WASM_DIR}"
mkdir -p "src/vault-canister-backend"
mkdir -p "src/shared-vault-canister-backend"

echo "[fetch] Downloading WASMs from release ${TAG}…"
curl -fsSL "${BASE}/vault_canister_backend.wasm" -o "${VAULT_WASM}"
curl -fsSL "${BASE}/shared_vault_canister_backend.wasm" -o "${SHARED_WASM}"

echo "[fetch] Downloading DIDs (if present in release)…"
VAULT_DID_OK=0
SHARED_DID_OK=0
if curl -fsSL "${BASE}/vault-canister-backend.did" -o "${VAULT_DID_PATH}"; then
  VAULT_DID_OK=1
fi
if curl -fsSL "${BASE}/shared-vault-canister-backend.did" -o "${SHARED_DID_PATH}"; then
  SHARED_DID_OK=1
fi

# Fallback: generate DID from wasm when not provided
if command -v candid-extractor >/dev/null 2>&1; then
  if [ "${VAULT_DID_OK}" -eq 0 ]; then
    echo "[fetch] Release did missing; extracting vault DID from wasm…"
    candid-extractor "${VAULT_WASM}" > "${VAULT_DID_PATH}"
  fi
  if [ "${SHARED_DID_OK}" -eq 0 ]; then
    echo "[fetch] Release did missing; extracting shared-vault DID from wasm…"
    candid-extractor "${SHARED_WASM}" > "${SHARED_DID_PATH}"
  fi
else
  if [ "${VAULT_DID_OK}" -eq 0 ] || [ "${SHARED_DID_OK}" -eq 0 ]; then
    echo "error: candid-extractor not found and one or more DIDs not available in the release." >&2
    exit 1
  fi
fi

# sanity check
test -s "${VAULT_WASM}" && test -s "${SHARED_WASM}" && test -s "${VAULT_DID_PATH}" && test -s "${SHARED_DID_PATH}"
echo "[fetch] OK."
