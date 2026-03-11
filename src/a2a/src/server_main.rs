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

//! Standalone A2A server binary.
//!
//! Starts an HTTP server exposing the A2A Agent Card and JSON-RPC endpoint.
//! Requires a running Briolette infrastructure (registrar, clerk, mint, validate).

use briolette_http_common::AppState;
use briolette_receiver::server::BrioletteReceiver;
use std::sync::Arc;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let registrar_uri = std::env::var("BRIOLETTE_REGISTRAR_URI")
        .unwrap_or_else(|_| "http://[::1]:50051".to_string());
    let clerk_uri =
        std::env::var("BRIOLETTE_CLERK_URI").unwrap_or_else(|_| "http://[::1]:50052".to_string());
    let mint_uri =
        std::env::var("BRIOLETTE_MINT_URI").unwrap_or_else(|_| "http://[::1]:50053".to_string());
    let validate_uri = std::env::var("BRIOLETTE_VALIDATE_URI")
        .unwrap_or_else(|_| "http://[::1]:50054".to_string());
    let http_addr = std::env::var("BRIOLETTE_A2A_ADDR").unwrap_or_else(|_| "0.0.0.0:8080".to_string());
    let base_url =
        std::env::var("BRIOLETTE_A2A_BASE_URL").unwrap_or_else(|_| format!("http://{}", http_addr));

    eprintln!("Initializing Briolette receiver...");
    let receiver = BrioletteReceiver::new(registrar_uri, clerk_uri, mint_uri, validate_uri)
        .await
        .map_err(|e| format!("Failed to initialize receiver: {:?}", e))?;

    let app_state = AppState {
        receiver: Arc::new(receiver),
        wallet: None,
    };

    let app = briolette_a2a::routes::router(app_state, base_url);

    let addr: std::net::SocketAddr = http_addr.parse()?;
    eprintln!("A2A server listening on {}", addr);
    axum::Server::bind(&addr)
        .serve(app.into_make_service_with_connect_info::<std::net::SocketAddr>())
        .await?;

    Ok(())
}
