# Getting Started with Briolette

This guide walks through setting up and running a local briolette system,
creating wallets, and performing token transfers.

## Prerequisites

- **Rust** 1.68+ ([install](https://www.rust-lang.org/tools/install))
- **protobuf compiler** (`apt install protobuf-compiler` or `brew install protobuf`)
- Approximately 2 GB disk space for compilation

## Quick Start (Automated)

The fastest way to try briolette:

```bash
# Clone and build
git clone <repo-url> briolette
cd briolette
cargo build

# Run the full demo (starts services, creates wallets, transfers tokens)
./scripts/quickstart.sh
```

This will:
1. Start all 7 briolette services
2. Create two wallets (Alice and Bob)
3. Mint 5 tokens for Alice
4. Transfer 2 tokens from Alice to Bob
5. Leave the services running for you to experiment

## Manual Setup

### Step 1: Build

```bash
cd briolette
cargo build
```

### Step 2: Start Services

Services must be started in dependency order. From the `src/` directory:

```bash
cd src
source utils.sh
start_servers
```

This starts:
| Service    | Port  | Purpose                          |
|------------|-------|----------------------------------|
| Registrar  | 50051 | Wallet registration & credentials|
| Clerk      | 50052 | Ticket issuance & epochs         |
| Mint       | 50053 | Token creation                   |
| TokenMap   | 50054 | Token history & double-spend DB  |
| Validate   | 50055 | Token chain verification         |
| Receiver   | 50056 | Transaction endpoint             |

### Step 3: Create a Wallet

```bash
# Initialize a wallet named "alice"
./target/debug/briolette-wallet-cli init --name alice

# Check the balance
./target/debug/briolette-wallet-cli balance --name alice
```

### Step 4: Get Tokens

```bash
# Withdraw 5 tokens from the mint
./target/debug/briolette-wallet-cli withdraw --name alice --amount 5

# Verify the balance
./target/debug/briolette-wallet-cli balance --name alice
```

### Step 5: Transfer Tokens

```bash
# Create a second wallet
./target/debug/briolette-wallet-cli init --name bob

# Export Bob's receiving ticket
BOB_TICKET=$(./target/debug/briolette-wallet-cli receive --name bob)

# Alice sends 2 tokens to Bob
./target/debug/briolette-wallet-cli send --name alice --amount 2 --to $BOB_TICKET

# Check both balances
./target/debug/briolette-wallet-cli balance --name alice
./target/debug/briolette-wallet-cli balance --name bob
```

### Step 6: Validate Tokens

```bash
# Verify tokens aren't double-spent (requires online connection to validate server)
./target/debug/briolette-wallet-cli validate --name alice
```

## Wallet CLI Reference

```
briolette-wallet-cli <COMMAND> [OPTIONS]

COMMANDS:
  init       Create and register a new wallet
  balance    Show token balance
  sync       Fetch latest epoch data
  tickets    Request new receiving tickets
  withdraw   Mint new tokens
  send       Transfer tokens to a recipient
  receive    Export a ticket for receiving tokens
  validate   Verify held tokens against the network
  info       Show wallet details

OPTIONS:
  --name <NAME>    Wallet name (default: 'default')
  --amount <N>     Token amount for withdraw/send
  --to <HEX>       Recipient ticket (hex) for send
  --count <N>      Number of tickets to request
```

### Environment Variables

| Variable              | Default                    | Description              |
|-----------------------|----------------------------|--------------------------|
| `BRIOLETTE_WALLET_DIR`| `.` (current directory)    | Wallet file storage      |
| `BRIOLETTE_REGISTRAR` | `http://127.0.0.1:50051`   | Registrar service URI    |
| `BRIOLETTE_CLERK`     | `http://127.0.0.1:50052`   | Clerk service URI        |
| `BRIOLETTE_MINT`      | `http://127.0.0.1:50053`   | Mint service URI         |
| `BRIOLETTE_VALIDATE`  | `http://127.0.0.1:50055`   | Validate service URI     |

## Docker

### Build

```bash
docker build -t briolette .
```

### Run with Docker Compose

```bash
# Start all services
docker compose up -d

# Run the demo transaction
docker compose --profile demo up demo

# View logs
docker compose logs -f

# Stop
docker compose down
```

## Architecture

When you run the quickstart, here's what happens:

```
┌─────────────────────────────────────────────────────────────┐
│                    Your Machine                              │
│                                                              │
│  ┌──────────┐  ┌───────┐  ┌──────┐  ┌──────────┐           │
│  │Registrar │  │ Clerk │  │ Mint │  │ TokenMap │            │
│  │  :50051  │  │:50052 │  │:50053│  │  :50054  │            │
│  └──────────┘  └───────┘  └──────┘  └──────────┘            │
│       │             │          │          │                   │
│  ┌──────────┐  ┌──────────┐                                  │
│  │ Validate │  │ Receiver │                                  │
│  │  :50055  │  │  :50056  │                                  │
│  └──────────┘  └──────────┘                                  │
│       ▲             ▲                                        │
│       │             │                                        │
│  ┌────┴─────────────┴────┐                                   │
│  │    Wallet CLI          │                                   │
│  │  (briolette-wallet-cli)│                                   │
│  └────────────────────────┘                                   │
│                                                              │
│  Wallet files: alice.wallet.json, bob.wallet.json            │
└─────────────────────────────────────────────────────────────┘
```

The wallet CLI connects to the services via gRPC. All token transfers
happen through ECDAA signatures — the tokens themselves are
cryptographically signed chains that can be verified offline by any
participant.

## What's Happening Under the Hood

1. **init**: Generates ECDAA keypairs, registers with the registrar
   (gets network + transfer credentials), syncs the epoch, and
   requests receiving tickets from the clerk.

2. **withdraw**: Uses a ticket to request freshly minted tokens from
   the mint. Each token is a signed chain starting with the mint's
   signature.

3. **send**: Creates an ECDAA transfer signature binding the token to
   the recipient's ticket. The recipient can verify this signature
   offline using the group public key.

4. **validate**: Sends token copies to the validate server, which
   checks the tokenmap for double-spending. Does not transfer
   ownership.

## Troubleshooting

**"Failed to register with the network"**
- Is the registrar running? Check: `curl -s http://127.0.0.1:50051 || echo "not running"`
- The registrar must be started first and generates keys on first run.

**"Failed to synchronize epoch"**
- The clerk needs an epoch generated. Run `briolette-clerk-generate-epoch` from `src/clerk/`.
- The tokenmap must be running for epoch generation to succeed.

**"No tickets available"**
- Run `briolette-wallet-cli tickets --name <wallet> --count 10`

**"Insufficient tokens"**
- Run `briolette-wallet-cli withdraw --name <wallet> --amount <N>`

## Next Steps

- Read the [theory of operation](design/theory_of_operation.md) for protocol details
- Read the [design concepts](design/concepts.md) for the design rationale
- See [bitcoin_l2.md](design/bitcoin_l2.md) for Bitcoin L2 bridge design
- Run the [simulation](../src/simulation/briolettesim/README.md) for large-scale modeling
