// Copyright 2025 The Briolette Authors.
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

//! Briolette Gateway — runs gRPC and HTTP servers side by side.
//!
//! The gateway serves:
//! - **gRPC** (default :50056): The existing Receiver protocol for wallet-to-wallet transfers
//! - **HTTP** (default :8080): A2A Agent Card + JSON-RPC, and x402 paywalled endpoints
//!
//! Both servers share the same BrioletteReceiver instance, ensuring consistent state.

use briolette_http_common::AppState;
use briolette_proto::briolette::receiver::receiver_server::ReceiverServer;
use briolette_proto::briolette::token;
use briolette_receiver::server::BrioletteReceiver;
use briolette_wallet::Wallet;
use briolette_x402::facilitator::Facilitator;
use briolette_x402::middleware::{PaywallConfig, PaywallLayer};
use briolette_x402::scheme;
use briolette_x402::types::{AmountJson, PaymentRequirements};
use clap::Parser as ClapParser;
use log::*;
use std::sync::{Arc, RwLock};

use axum::{
    body::{self, BoxBody, Full},
    http::{Response, StatusCode},
    routing::get,
    Router,
};

#[derive(ClapParser, Debug)]
#[command(author, version, about = "Briolette Gateway: gRPC + A2A + x402")]
struct Args {
    /// gRPC listen address
    #[arg(long, default_value = "[::1]:50056")]
    grpc_addr: String,

    /// HTTP listen address
    #[arg(long, default_value = "0.0.0.0:8080")]
    http_addr: String,

    /// Base URL for Agent Card (used in A2A discovery)
    #[arg(long, default_value = "http://localhost:8080")]
    base_url: String,

    /// Registrar server URI
    #[arg(short = 'r', long, default_value = "http://[::1]:50051")]
    registrar_uri: String,

    /// Clerk server URI
    #[arg(short = 'c', long, default_value = "http://[::1]:50052")]
    clerk_uri: String,

    /// Mint server URI
    #[arg(short = 'm', long, default_value = "http://[::1]:50053")]
    mint_uri: String,

    /// Validate server URI
    #[arg(short = 'v', long, default_value = "http://[::1]:50055")]
    validate_uri: String,

    /// Default payment amount (whole units) for x402 demo endpoints
    #[arg(long, default_value = "1")]
    paywall_amount: i32,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    stderrlog::new()
        .quiet(false)
        .verbosity(2)
        .timestamp(stderrlog::Timestamp::Millisecond)
        .init()
        .unwrap();

    let args = Args::parse();

    info!("Initializing Briolette receiver...");
    let mut receiver = BrioletteReceiver::new(
        args.registrar_uri.clone(),
        args.clerk_uri.clone(),
        args.mint_uri.clone(),
        args.validate_uri.clone(),
    )
    .await
    .map_err(|e| format!("Failed to initialize receiver: {:?}", e))?;
    receiver.next_amount(args.paywall_amount as u64, 0);

    let receiver = Arc::new(receiver);

    // Create a wallet for the facilitator (shares credentials with the receiver's wallet).
    let facilitator_wallet = {
        let mut wd = briolette_wallet::WalletData::new(
            args.registrar_uri,
            args.clerk_uri,
            args.mint_uri,
            args.validate_uri,
        )
        .map_err(|e| format!("Failed to create facilitator wallet: {:?}", e))?;
        wd.initialize_keys(b"gateway-facilitator-001");
        wd.initialize_credential().await;
        wd.synchronize().await;
        Arc::new(RwLock::new(wd))
    };

    let app_state = AppState {
        receiver: receiver.clone(),
        wallet: Some(facilitator_wallet.clone()),
    };

    // ── gRPC server ─────────────────────────────────────────────────────
    let grpc_addr: std::net::SocketAddr = args.grpc_addr.parse()?;
    let grpc_receiver = (*receiver).clone();
    let grpc_handle = tokio::spawn(async move {
        info!("gRPC server listening on {}", grpc_addr);
        tonic::transport::Server::builder()
            .add_service(ReceiverServer::new(grpc_receiver))
            .serve(grpc_addr)
            .await
            .expect("gRPC server failed");
    });

    // ── HTTP server (A2A + x402) ────────────────────────────────────────

    // A2A routes: /.well-known/agent.json + /a2a
    let a2a_router = briolette_a2a::routes::router(app_state.clone(), args.base_url);

    // x402 demo: a paywalled endpoint at /api/data
    let facilitator = Facilitator::new(facilitator_wallet);
    let paywall_config = PaywallConfig {
        requirements: PaymentRequirements {
            scheme: scheme::SCHEME_NAME.to_string(),
            network: "testnet".to_string(),
            pay_to: serde_json::json!({}), // Filled dynamically per-request in production
            max_amount_required: AmountJson {
                whole: args.paywall_amount,
                fractional: 0,
                code: 0,
            },
            resource: "/api/data".to_string(),
            description: Some("Access to data endpoint".to_string()),
            extra: None,
        },
        required_amount: token::Amount {
            whole: args.paywall_amount,
            fractional: 0,
            code: token::AmountType::TestToken.into(),
        },
    };

    let x402_routes = Router::new()
        .route("/api/data", get(demo_data_handler))
        .layer(PaywallLayer::new(facilitator, paywall_config));

    let http_app = a2a_router.merge(x402_routes);

    let http_addr: std::net::SocketAddr = args.http_addr.parse()?;
    let http_handle = tokio::spawn(async move {
        info!("HTTP server listening on {}", http_addr);
        axum::Server::bind(&http_addr)
            .serve(http_app.into_make_service_with_connect_info::<std::net::SocketAddr>())
            .await
            .expect("HTTP server failed");
    });

    // Run both servers concurrently.
    tokio::select! {
        _ = grpc_handle => { error!("gRPC server exited unexpectedly"); }
        _ = http_handle => { error!("HTTP server exited unexpectedly"); }
    }

    Ok(())
}

/// Demo handler for a paywalled resource.
async fn demo_data_handler() -> Response<BoxBody> {
    let data = serde_json::json!({
        "message": "This is paid content, delivered via x402.",
    });

    Response::builder()
        .status(StatusCode::OK)
        .header("content-type", "application/json")
        .body(body::boxed(Full::from(
            serde_json::to_string(&data).unwrap(),
        )))
        .unwrap()
}
