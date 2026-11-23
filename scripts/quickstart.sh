#!/bin/bash
# Briolette Quickstart Script
#
# This script starts all briolette services locally, initializes two wallets,
# and performs a demo token transfer between them.
#
# Prerequisites:
#   - Rust toolchain (1.68+)
#   - Workspace built: cargo build (from repo root)
#
# Usage:
#   ./scripts/quickstart.sh

set -e

REPO_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
TARGET="${REPO_ROOT}/target/debug"
SRC="${REPO_ROOT}/src"
WALLET_DIR="${REPO_ROOT}/demo_wallets"
PIDS=()

cleanup() {
    echo ""
    echo "Shutting down services..."
    for pid in "${PIDS[@]}"; do
        kill "$pid" 2>/dev/null || true
    done
    wait 2>/dev/null || true
    echo "Done."
}
trap cleanup EXIT

wait_for_port() {
    local port=$1
    local name=$2
    local max_wait=30
    local waited=0
    while ! (echo >/dev/tcp/127.0.0.1/$port) 2>/dev/null; do
        sleep 0.5
        waited=$((waited + 1))
        if [ $waited -ge $max_wait ]; then
            echo "ERROR: $name (port $port) failed to start after ${max_wait}s"
            exit 1
        fi
    done
    echo "  $name ready (port $port)"
}

echo "============================================================"
echo "  Briolette Quickstart"
echo "============================================================"
echo ""

# Check that binaries exist
if [ ! -f "${TARGET}/briolette-registrar-server" ]; then
    echo "Building workspace (this may take a few minutes)..."
    cargo build --manifest-path "${REPO_ROOT}/Cargo.toml"
fi

# Create wallet directory
mkdir -p "${WALLET_DIR}"

echo "Starting services..."
echo ""

# 1. Registrar (generates issuer keys on first run)
cd "${SRC}/registrar"
"${TARGET}/briolette-registrar-server" &
PIDS+=($!)
wait_for_port 50051 "Registrar"

# 2. Register a wallet (populates credential files)
"${TARGET}/briolette-registrar-client" &
REGCLIENT=$!
wait $REGCLIENT || true

# 3. Clerk (ticket server, needs registrar keys)
cd "${SRC}/clerk"
"${TARGET}/briolette-clerk-server" &
PIDS+=($!)
wait_for_port 50052 "Clerk"

# 4. TokenMap (token history database)
cd "${SRC}/tokenmap"
"${TARGET}/briolette-tokenmap-server" &
PIDS+=($!)
wait_for_port 50054 "TokenMap"

# 5. Generate initial epoch (needs clerk + tokenmap)
cd "${SRC}/clerk"
"${TARGET}/briolette-clerk-generate-epoch" &
EPOCHGEN=$!
wait $EPOCHGEN || true
echo "  Epoch generated"

# Wait for epoch to propagate
sleep 2

# 6. Mint (token creation)
cd "${SRC}/mint"
"${TARGET}/briolette-mint-server" &
PIDS+=($!)
wait_for_port 50053 "Mint"

# 7. Validate (token verification)
cd "${SRC}/validate"
"${TARGET}/briolette-validate-server" &
PIDS+=($!)
wait_for_port 50055 "Validate"

# 8. Receiver (transaction endpoint)
cd "${SRC}/receiver"
"${TARGET}/briolette-receiver-server" &
PIDS+=($!)
wait_for_port 50056 "Receiver"

echo ""
echo "All services running."
echo ""
echo "============================================================"
echo "  Wallet Demo"
echo "============================================================"
echo ""

export BRIOLETTE_WALLET_DIR="${WALLET_DIR}"
WALLET_CLI="${TARGET}/briolette-wallet-cli"

# Initialize Alice's wallet
echo "--- Initializing Alice's wallet ---"
"${WALLET_CLI}" init --name alice
echo ""

# Initialize Bob's wallet
echo "--- Initializing Bob's wallet ---"
"${WALLET_CLI}" init --name bob
echo ""

# Alice withdraws 5 tokens from the mint
echo "--- Alice withdraws 5 tokens ---"
"${WALLET_CLI}" withdraw --name alice --amount 5
echo ""

# Show balances
echo "--- Balances ---"
"${WALLET_CLI}" balance --name alice
echo ""
"${WALLET_CLI}" balance --name bob
echo ""

# Bob exports a receiving ticket
echo "--- Bob exports a receiving ticket ---"
BOB_TICKET=$("${WALLET_CLI}" receive --name bob)
echo "  Ticket: ${BOB_TICKET:0:40}..."
echo ""

# Alice sends 2 tokens to Bob
echo "--- Alice sends 2 tokens to Bob ---"
"${WALLET_CLI}" send --name alice --amount 2 --to "${BOB_TICKET}"
echo ""

# Show final balances
echo "--- Final Balances ---"
"${WALLET_CLI}" balance --name alice
echo ""
"${WALLET_CLI}" balance --name bob
echo ""

echo "============================================================"
echo "  Demo Complete!"
echo "============================================================"
echo ""
echo "The services are still running. You can interact with them:"
echo ""
echo "  export BRIOLETTE_WALLET_DIR=${WALLET_DIR}"
echo "  ${WALLET_CLI} balance --name alice"
echo "  ${WALLET_CLI} withdraw --name alice --amount 3"
echo "  ${WALLET_CLI} validate --name alice"
echo ""
echo "Press Ctrl+C to stop all services."
echo ""

# Wait for Ctrl+C
wait
