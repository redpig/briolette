#!/bin/bash
# Briolette service entrypoint.
#
# Usage: entrypoint.sh <service-name>
#
# Each service expects its data directory at /briolette/data/<service>/
# and reads peer service addresses from environment variables.

set -e

SERVICE="${1:-help}"
DATA_DIR="/briolette/data"

case "$SERVICE" in
  registrar)
    cd "${DATA_DIR}/registrar"
    exec briolette-registrar-server
    ;;

  registrar-client)
    cd "${DATA_DIR}/registrar"
    exec briolette-registrar-client
    ;;

  clerk)
    cd "${DATA_DIR}/clerk"
    exec briolette-clerk-server
    ;;

  clerk-generate-epoch)
    cd "${DATA_DIR}/clerk"
    exec briolette-clerk-generate-epoch
    ;;

  tokenmap)
    cd "${DATA_DIR}/tokenmap"
    exec briolette-tokenmap-server
    ;;

  mint)
    cd "${DATA_DIR}/mint"
    exec briolette-mint-server
    ;;

  validate)
    cd "${DATA_DIR}/validate"
    exec briolette-validate-server
    ;;

  receiver)
    cd "${DATA_DIR}/receiver"
    exec briolette-receiver-server
    ;;

  bridge)
    cd "${DATA_DIR}/bridge"
    exec briolette-bridge-server
    ;;

  wallet-cli)
    shift
    exec briolette-wallet-cli "$@"
    ;;

  bootstrap)
    # Run the full bootstrap sequence:
    # 1. Start registrar, register a wallet
    # 2. Start clerk, generate initial epoch
    # 3. Start remaining services
    echo "Bootstrap mode — use docker compose instead"
    exit 1
    ;;

  help|*)
    echo "Briolette service entrypoint"
    echo ""
    echo "Usage: entrypoint.sh <service>"
    echo ""
    echo "Services:"
    echo "  registrar           Credential registration server (port 50051)"
    echo "  registrar-client    Register a wallet with the registrar"
    echo "  clerk               Ticket issuance server (port 50052)"
    echo "  clerk-generate-epoch Generate initial epoch data"
    echo "  tokenmap            Token history database (port 50054)"
    echo "  mint                Token minting server (port 50053)"
    echo "  validate            Token validation server (port 50055)"
    echo "  receiver            Transaction receiver server (port 50056)"
    echo "  bridge              L1 bridge server (port 50057)"
    echo "  wallet-cli          Interactive wallet CLI"
    exit 0
    ;;
esac
