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

use briolette_recovery::server::BrioletteRecovery;
use clap::Parser as ClapParser;
use log::*;
use tokio;

use briolette_proto::briolette::recovery::recovery_server::RecoveryServer;

#[derive(ClapParser, Debug)]
#[command(author, version, about = "Briolette Recovery Server", long_about = None)]
struct Args {
    // Address to listen on
    #[arg(
        short = 'l',
        long,
        value_name = "IP:PORT",
        default_value = "[::1]:50058"
    )]
    listen_address: String,
    // Registrar server URI
    #[arg(
        short = 'r',
        long,
        value_name = "URI",
        default_value = "http://[::1]:50051"
    )]
    registrar_uri: String,
    // Mint server URI
    #[arg(
        short = 'm',
        long,
        value_name = "URI",
        default_value = "http://[::1]:50053"
    )]
    mint_uri: String,
    // Clerk server URI
    #[arg(
        short = 'c',
        long,
        value_name = "URI",
        default_value = "http://[::1]:50052"
    )]
    clerk_uri: String,
    // Validate server URI
    #[arg(
        short = 'v',
        long,
        value_name = "URI",
        default_value = "http://[::1]:50055"
    )]
    validate_uri: String,
    // TokenMap server URI
    #[arg(
        short = 't',
        long,
        value_name = "URI",
        default_value = "http://[::1]:50056"
    )]
    tokenmap_uri: String,
    // Path to the SQLite database for binding storage
    #[arg(
        short = 'd',
        long,
        value_name = "PATH",
        default_value = "data/recovery/bindings.db"
    )]
    db_path: String,
    // Mandatory cooling-off period (epochs) between token expiry and recovery
    #[arg(long, value_name = "EPOCHS", default_value = "2")]
    cooloff_epochs: u64,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    stderrlog::new()
        .quiet(false)
        .verbosity(1)
        .timestamp(stderrlog::Timestamp::Millisecond)
        .init()
        .unwrap();
    let args = Args::parse();

    // Ensure the database directory exists
    if let Some(parent) = std::path::Path::new(&args.db_path).parent() {
        std::fs::create_dir_all(parent)?;
    }

    let addr = args.listen_address.parse().unwrap();
    info!("Setting up recovery server...");
    let recovery = BrioletteRecovery::new(
        args.registrar_uri,
        args.clerk_uri,
        args.mint_uri,
        args.validate_uri,
        args.tokenmap_uri,
        &args.db_path,
        args.cooloff_epochs,
    )
    .await
    .unwrap();
    info!("Recovery server listening on {}", addr);
    tonic::transport::Server::builder()
        .add_service(RecoveryServer::new(recovery))
        .serve(addr)
        .await?;
    Ok(())
}
