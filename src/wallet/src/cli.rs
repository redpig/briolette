// Copyright 2023 The Briolette Authors.
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

//! Briolette Wallet CLI
//!
//! Interactive command-line wallet for the Briolette digital currency system.
//!
//! # Quick start
//!
//! ```bash
//! # Initialize a new wallet and register with the network
//! briolette-wallet-cli init --name alice
//!
//! # Check balance
//! briolette-wallet-cli balance
//!
//! # Withdraw tokens from the mint
//! briolette-wallet-cli withdraw --amount 5
//!
//! # Export a receiving address (ticket)
//! briolette-wallet-cli receive
//!
//! # Send tokens to another wallet's ticket
//! briolette-wallet-cli send --amount 2 --to <ticket-hex>
//!
//! # Validate held tokens against the network
//! briolette-wallet-cli validate
//! ```

use briolette_wallet::{Wallet, WalletData};
use briolette_proto::briolette::token;
use prost::Message;
use std::path::PathBuf;

const DEFAULT_REGISTRAR_URI: &str = "http://127.0.0.1:50051";
const DEFAULT_CLERK_URI: &str = "http://127.0.0.1:50052";
const DEFAULT_MINT_URI: &str = "http://127.0.0.1:50053";
const DEFAULT_VALIDATE_URI: &str = "http://127.0.0.1:50055";

fn wallet_path(name: &str) -> PathBuf {
    let dir = std::env::var("BRIOLETTE_WALLET_DIR")
        .unwrap_or_else(|_| ".".to_string());
    PathBuf::from(dir).join(format!("{}.wallet.json", name))
}

fn load_wallet(name: &str) -> Result<WalletData, String> {
    let path = wallet_path(name);
    WalletData::load(&path).map_err(|_| {
        format!(
            "Could not load wallet '{}' from {}. Run 'init' first.",
            name,
            path.display()
        )
    })
}

fn save_wallet(name: &str, wallet: &WalletData) -> Result<(), String> {
    let path = wallet_path(name);
    if wallet.store(&path) {
        Ok(())
    } else {
        Err(format!("Failed to save wallet to {}", path.display()))
    }
}

fn print_help() {
    println!("briolette-wallet-cli — Briolette digital currency wallet");
    println!();
    println!("USAGE:");
    println!("  briolette-wallet-cli <COMMAND> [OPTIONS]");
    println!();
    println!("COMMANDS:");
    println!("  init       Create a new wallet and register with the network");
    println!("  balance    Show token balance");
    println!("  sync       Synchronize with the latest epoch");
    println!("  tickets    Request new receiving tickets from the clerk");
    println!("  withdraw   Withdraw tokens from the mint");
    println!("  send       Transfer tokens to a recipient's ticket");
    println!("  receive    Export a ticket for receiving tokens");
    println!("  refresh    Refresh expiring tickets (preserves payment pseudonym)");
    println!("  validate   Validate held tokens against the network");
    println!("  info       Show wallet details");
    println!("  help       Show this help message");
    println!();
    println!("COMMON OPTIONS:");
    println!("  --name <NAME>           Wallet name (default: 'default')");
    println!("  --registrar <URI>       Registrar URI (default: {DEFAULT_REGISTRAR_URI})");
    println!("  --clerk <URI>           Clerk URI (default: {DEFAULT_CLERK_URI})");
    println!("  --mint <URI>            Mint URI (default: {DEFAULT_MINT_URI})");
    println!("  --validate-uri <URI>    Validate URI (default: {DEFAULT_VALIDATE_URI})");
    println!();
    println!("ENVIRONMENT:");
    println!("  BRIOLETTE_WALLET_DIR    Directory for wallet files (default: current dir)");
    println!("  BRIOLETTE_REGISTRAR     Registrar URI override");
    println!("  BRIOLETTE_CLERK         Clerk URI override");
    println!("  BRIOLETTE_MINT          Mint URI override");
    println!("  BRIOLETTE_VALIDATE      Validate URI override");
    println!();
    println!("EXAMPLES:");
    println!("  # Set up two wallets and transfer between them:");
    println!("  briolette-wallet-cli init --name alice");
    println!("  briolette-wallet-cli init --name bob");
    println!("  briolette-wallet-cli withdraw --name alice --amount 5");
    println!("  briolette-wallet-cli receive --name bob > bob_ticket.hex");
    println!("  briolette-wallet-cli send --name alice --amount 2 --to $(cat bob_ticket.hex)");
}

struct Config {
    name: String,
    registrar_uri: String,
    clerk_uri: String,
    mint_uri: String,
    validate_uri: String,
}

impl Config {
    fn from_args(args: &[String]) -> Self {
        let mut name = "default".to_string();
        let mut registrar_uri = std::env::var("BRIOLETTE_REGISTRAR")
            .unwrap_or_else(|_| DEFAULT_REGISTRAR_URI.to_string());
        let mut clerk_uri = std::env::var("BRIOLETTE_CLERK")
            .unwrap_or_else(|_| DEFAULT_CLERK_URI.to_string());
        let mut mint_uri = std::env::var("BRIOLETTE_MINT")
            .unwrap_or_else(|_| DEFAULT_MINT_URI.to_string());
        let mut validate_uri = std::env::var("BRIOLETTE_VALIDATE")
            .unwrap_or_else(|_| DEFAULT_VALIDATE_URI.to_string());

        let mut i = 0;
        while i < args.len() {
            match args[i].as_str() {
                "--name" => {
                    i += 1;
                    if i < args.len() {
                        name = args[i].clone();
                    }
                }
                "--registrar" => {
                    i += 1;
                    if i < args.len() {
                        registrar_uri = args[i].clone();
                    }
                }
                "--clerk" => {
                    i += 1;
                    if i < args.len() {
                        clerk_uri = args[i].clone();
                    }
                }
                "--mint" => {
                    i += 1;
                    if i < args.len() {
                        mint_uri = args[i].clone();
                    }
                }
                "--validate-uri" => {
                    i += 1;
                    if i < args.len() {
                        validate_uri = args[i].clone();
                    }
                }
                _ => {}
            }
            i += 1;
        }

        Self {
            name,
            registrar_uri,
            clerk_uri,
            mint_uri,
            validate_uri,
        }
    }
}

fn get_arg(args: &[String], flag: &str) -> Option<String> {
    args.iter()
        .position(|a| a == flag)
        .and_then(|i| args.get(i + 1))
        .cloned()
}

async fn cmd_init(config: &Config) -> Result<(), String> {
    let path = wallet_path(&config.name);
    if path.exists() {
        return Err(format!(
            "Wallet '{}' already exists at {}. Delete it first to reinitialize.",
            config.name,
            path.display()
        ));
    }

    println!("Creating wallet '{}'...", config.name);

    let mut wallet = WalletData::new(
        config.registrar_uri.clone(),
        config.clerk_uri.clone(),
        config.mint_uri.clone(),
        config.validate_uri.clone(),
    ).map_err(|e| format!("Failed to create wallet: {:?}", e))?;

    // Generate keys
    let hw_id = format!("briolette-wallet-{}", config.name);
    if !wallet.initialize_keys(hw_id.as_bytes()) {
        return Err("Failed to generate wallet keys".to_string());
    }
    println!("  Keys generated");

    // Register with the network
    if !wallet.initialize_credential().await {
        return Err("Failed to register with the network. Is the registrar running?".to_string());
    }
    println!("  Registered with network");

    // Synchronize epoch
    if !wallet.synchronize().await {
        return Err("Failed to synchronize epoch. Is the clerk running?".to_string());
    }
    println!("  Epoch synchronized");

    // Get initial tickets
    if !wallet.get_tickets(10).await {
        return Err("Failed to get tickets. Is the clerk running?".to_string());
    }
    println!("  Got 10 receiving tickets");

    // Save
    save_wallet(&config.name, &wallet)?;
    println!();
    println!("Wallet '{}' created at {}", config.name, path.display());
    println!("  Tickets: {}", wallet.tickets.len());
    println!("  Tokens:  {}", wallet.tokens.len());
    Ok(())
}

async fn cmd_balance(config: &Config) -> Result<(), String> {
    let wallet = load_wallet(&config.name)?;
    let mut total_whole: i64 = 0;
    let mut total_frac: i64 = 0;

    for entry in &wallet.tokens {
        total_whole += entry.whole_value as i64;
        total_frac += entry.fractional_value as i64;
    }

    // Normalize fractional overflow
    total_whole += total_frac / 1_000_000;
    total_frac %= 1_000_000;

    println!("Wallet: {}", config.name);
    println!("  Tokens:     {}", wallet.tokens.len());
    println!("  Balance:    {}.{:06}", total_whole, total_frac.abs());
    println!("  Tickets:    {} remaining", wallet.tickets.len());
    println!("  Pending:    {} tokens to send", wallet.pending_tokens.len());
    Ok(())
}

async fn cmd_sync(config: &Config) -> Result<(), String> {
    let mut wallet = load_wallet(&config.name)?;

    if !wallet.synchronize().await {
        return Err("Failed to synchronize. Is the clerk running?".to_string());
    }

    save_wallet(&config.name, &wallet)?;
    println!("Epoch synchronized (epoch {})", wallet.epoch.epoch);
    Ok(())
}

async fn cmd_tickets(config: &Config, args: &[String]) -> Result<(), String> {
    let mut wallet = load_wallet(&config.name)?;
    let count: u32 = get_arg(args, "--count")
        .unwrap_or_else(|| "5".to_string())
        .parse()
        .map_err(|_| "Invalid count")?;

    if !wallet.get_tickets(count).await {
        return Err("Failed to get tickets. Is the clerk running?".to_string());
    }

    save_wallet(&config.name, &wallet)?;
    println!("Got {} new tickets ({} total)", count, wallet.tickets.len());
    Ok(())
}

async fn cmd_refresh(config: &Config, args: &[String]) -> Result<(), String> {
    use briolette_proto::briolette::token::TicketExpiry;

    let mut wallet = load_wallet(&config.name)?;

    if wallet.tickets.is_empty() {
        return Err("No tickets to refresh. Run 'tickets' first.".to_string());
    }

    // Find tickets that are expiring soon (within 2 epochs) or already expired,
    // or refresh specific indices if --index is given.
    let indices: Vec<usize> = if let Some(idx_str) = get_arg(args, "--index") {
        // Refresh specific ticket(s) by index
        idx_str
            .split(',')
            .map(|s| s.trim().parse::<usize>().map_err(|_| "Invalid index"))
            .collect::<Result<Vec<_>, _>>()?
    } else {
        // Auto-detect: refresh tickets expiring within --threshold epochs (default: 2)
        let threshold: u64 = get_arg(args, "--threshold")
            .unwrap_or_else(|| "2".to_string())
            .parse()
            .map_err(|_| "Invalid threshold")?;
        let now = chrono::Utc::now().timestamp() as u64;
        let epoch_secs = 86400u64; // TODO: read from epoch data
        let cutoff = now + threshold * epoch_secs;

        wallet
            .tickets
            .iter()
            .enumerate()
            .filter(|(_, t)| {
                let st: briolette_proto::briolette::token::SignedTicket = (*t).clone().into();
                st.expires_on() <= cutoff
            })
            .map(|(i, _)| i)
            .collect()
    };

    if indices.is_empty() {
        println!("No tickets need refreshing (all have sufficient lifetime).");
        return Ok(());
    }

    println!("Refreshing {} ticket(s)...", indices.len());
    let refreshed = wallet.refresh_tickets(&indices).await;

    save_wallet(&config.name, &wallet)?;
    if refreshed > 0 {
        println!("Refreshed {} ticket(s) ({} total)", refreshed, wallet.tickets.len());
    } else {
        return Err("Failed to refresh tickets. Is the clerk running?".to_string());
    }
    Ok(())
}

async fn cmd_withdraw(config: &Config, args: &[String]) -> Result<(), String> {
    let mut wallet = load_wallet(&config.name)?;
    let amount: u32 = get_arg(args, "--amount")
        .ok_or("--amount is required")?
        .parse()
        .map_err(|_| "Invalid amount")?;

    if wallet.tickets.is_empty() {
        return Err("No tickets available. Run 'tickets' first.".to_string());
    }

    if !wallet.withdraw(amount).await {
        return Err("Failed to withdraw. Is the mint running?".to_string());
    }

    save_wallet(&config.name, &wallet)?;
    println!("Withdrew {} tokens ({} total)", amount, wallet.tokens.len());
    Ok(())
}

async fn cmd_send(config: &Config, args: &[String]) -> Result<(), String> {
    let mut wallet = load_wallet(&config.name)?;
    let amount: u32 = get_arg(args, "--amount")
        .ok_or("--amount is required")?
        .parse()
        .map_err(|_| "Invalid amount")?;
    let to_hex = get_arg(args, "--to")
        .ok_or("--to <ticket-hex> is required")?;

    let ticket_bytes = hex_decode(&to_hex)
        .map_err(|_| "Invalid hex in --to argument")?;

    if wallet.tokens.len() < amount as usize {
        return Err(format!(
            "Insufficient tokens: have {}, need {}",
            wallet.tokens.len(),
            amount
        ));
    }

    if !wallet.transfer(amount, ticket_bytes) {
        return Err("Transfer failed".to_string());
    }

    // Show the pending tokens as hex (the sender would transmit these to the receiver)
    println!("Transfer prepared: {} tokens", wallet.pending_tokens.len());
    for (i, pending) in wallet.pending_tokens.iter().enumerate() {
        println!("  Token {}: {} bytes", i, pending.len());
    }

    // In a real peer-to-peer flow, the sender would transmit pending_tokens
    // to the receiver. For the CLI, we output them as hex.
    println!();
    println!("Pending tokens (hex-encoded, transmit to receiver):");
    for pending in &wallet.pending_tokens {
        println!("{}", hex_encode(pending));
    }

    // Clear pending after displaying
    wallet.pending_tokens.clear();
    save_wallet(&config.name, &wallet)?;
    Ok(())
}

async fn cmd_receive(config: &Config) -> Result<(), String> {
    let wallet = load_wallet(&config.name)?;

    if wallet.tickets.is_empty() {
        return Err("No tickets available. Run 'tickets' first.".to_string());
    }

    // Export the first available ticket as hex
    let ticket_entry = &wallet.tickets[0];
    let signed_ticket: token::SignedTicket = ticket_entry.clone().into();
    let ticket_bytes = signed_ticket.encode_to_vec();

    // Print just the hex to stdout (for piping)
    println!("{}", hex_encode(&ticket_bytes));

    // Print info to stderr so piping works
    eprintln!("Exported ticket for wallet '{}' ({} tickets remaining)",
        config.name,
        wallet.tickets.len()
    );

    Ok(())
}

async fn cmd_validate(config: &Config) -> Result<(), String> {
    let wallet = load_wallet(&config.name)?;

    if wallet.tokens.is_empty() {
        println!("No tokens to validate.");
        return Ok(());
    }

    if !wallet.validate().await {
        return Err("Token validation failed! Some tokens may be double-spent.".to_string());
    }

    println!("All {} tokens validated successfully", wallet.tokens.len());
    Ok(())
}

async fn cmd_info(config: &Config) -> Result<(), String> {
    let wallet = load_wallet(&config.name)?;
    let path = wallet_path(&config.name);

    println!("Wallet: {}", config.name);
    println!("  File:       {}", path.display());
    println!("  Epoch:      {}", wallet.epoch.epoch);
    println!("  Tokens:     {}", wallet.tokens.len());
    println!("  Tickets:    {}", wallet.tickets.len());
    println!("  Pending:    {}", wallet.pending_tokens.len());

    if !wallet.tokens.is_empty() {
        let mut total_whole: i64 = 0;
        let mut total_frac: i64 = 0;
        for entry in &wallet.tokens {
            total_whole += entry.whole_value as i64;
            total_frac += entry.fractional_value as i64;
        }
        total_whole += total_frac / 1_000_000;
        total_frac %= 1_000_000;
        println!("  Balance:    {}.{:06}", total_whole, total_frac.abs());
    }

    println!();
    println!("Service endpoints:");
    println!("  Registrar:  {}", DEFAULT_REGISTRAR_URI);
    println!("  Clerk:      {}", DEFAULT_CLERK_URI);
    println!("  Mint:       {}", DEFAULT_MINT_URI);
    println!("  Validate:   {}", DEFAULT_VALIDATE_URI);
    Ok(())
}

// Minimal hex encode/decode to avoid adding a dependency
fn hex_encode(data: &[u8]) -> String {
    data.iter().map(|b| format!("{:02x}", b)).collect()
}

fn hex_decode(hex: &str) -> Result<Vec<u8>, String> {
    if hex.len() % 2 != 0 {
        return Err("odd length".to_string());
    }
    (0..hex.len())
        .step_by(2)
        .map(|i| {
            u8::from_str_radix(&hex[i..i + 2], 16)
                .map_err(|e| format!("invalid hex at {}: {}", i, e))
        })
        .collect()
}

#[tokio::main]
async fn main() {
    // Initialize logging
    let _ = stderrlog::new()
        .quiet(false)
        .verbosity(1)
        .timestamp(stderrlog::Timestamp::Off)
        .init();

    let args: Vec<String> = std::env::args().collect();

    if args.len() < 2 {
        print_help();
        std::process::exit(1);
    }

    let command = &args[1];
    let remaining: Vec<String> = args[2..].to_vec();
    let config = Config::from_args(&remaining);

    let result = match command.as_str() {
        "init" => cmd_init(&config).await,
        "balance" | "bal" => cmd_balance(&config).await,
        "sync" | "synchronize" => cmd_sync(&config).await,
        "tickets" | "ticket" => cmd_tickets(&config, &remaining).await,
        "refresh" => cmd_refresh(&config, &remaining).await,
        "withdraw" | "mint" => cmd_withdraw(&config, &remaining).await,
        "send" | "transfer" => cmd_send(&config, &remaining).await,
        "receive" | "recv" | "address" => cmd_receive(&config).await,
        "validate" | "verify" => cmd_validate(&config).await,
        "info" | "status" => cmd_info(&config).await,
        "help" | "--help" | "-h" => {
            print_help();
            Ok(())
        }
        other => Err(format!("Unknown command: '{}'. Run 'help' for usage.", other)),
    };

    if let Err(e) = result {
        eprintln!("Error: {}", e);
        std::process::exit(1);
    }
}
