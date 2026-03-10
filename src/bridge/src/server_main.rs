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

use briolette_bridge::deposit_processor::{DepositProcessor, DepositProcessorConfig, TicketRegistry};
use briolette_bridge::ethereum::MockEthereumClient;
use briolette_bridge::key_registry::{FileKeyRegistry, KeyMaterial, KeyRegistrySource};
use briolette_bridge::server::BrioletteBridge;
use briolette_proto::briolette::bridge::bridge_server::BridgeServer;
use briolette_proto::briolette::bridge::*;
use log::*;
use std::sync::Arc;
use tokio::sync::RwLock;
use tonic::{Request, Response, Status};

struct BridgeGrpc {
    inner: BrioletteBridge,
}

#[tonic::async_trait]
impl briolette_proto::briolette::bridge::bridge_server::Bridge for BridgeGrpc {
    async fn withdraw_to_l1(
        &self,
        request: Request<WithdrawRequest>,
    ) -> Result<Response<WithdrawReply>, Status> {
        let inner_request = request.into_inner();
        match self.inner.withdraw_to_l1_impl(&inner_request).await {
            Ok(reply) => Ok(Response::new(reply)),
            Err(e) => Err(e.into()),
        }
    }

    async fn get_deposit_status(
        &self,
        request: Request<DepositStatusRequest>,
    ) -> Result<Response<DepositStatusReply>, Status> {
        let inner_request = request.into_inner();
        match self.inner.get_deposit_status_impl(&inner_request).await {
            Ok(reply) => Ok(Response::new(reply)),
            Err(e) => Err(e.into()),
        }
    }

    async fn get_withdrawal_status(
        &self,
        request: Request<WithdrawalStatusRequest>,
    ) -> Result<Response<WithdrawalStatusReply>, Status> {
        let inner_request = request.into_inner();
        match self.inner.get_withdrawal_status_impl(&inner_request).await {
            Ok(reply) => Ok(Response::new(reply)),
            Err(e) => Err(e.into()),
        }
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    stderrlog::new()
        .quiet(false)
        .verbosity(2)
        .timestamp(stderrlog::Timestamp::Millisecond)
        .init()
        .unwrap();

    let addr = "127.0.0.1:50057".parse().unwrap();

    // Determine key source mode from environment.
    // BRIOLETTE_KEY_SOURCE=onchain requires the `alloy` feature and env vars:
    //   BRIOLETTE_RPC_URL, BRIOLETTE_BRIDGE_ADDRESS, BRIOLETTE_OPERATOR_KEY
    // Default: file-based keys (existing behavior).
    let key_source = std::env::var("BRIOLETTE_KEY_SOURCE").unwrap_or_default();

    let (eth_client, keys): (Arc<dyn briolette_bridge::ethereum::EthereumClient>, KeyMaterial) =
        if key_source == "onchain" {
            #[cfg(feature = "alloy")]
            {
                use briolette_bridge::alloy_client::AlloyEthereumClient;
                use briolette_bridge::key_registry::OnChainKeyRegistry;

                let rpc_url = std::env::var("BRIOLETTE_RPC_URL")
                    .expect("BRIOLETTE_RPC_URL required for onchain mode");
                let bridge_addr_hex = std::env::var("BRIOLETTE_BRIDGE_ADDRESS")
                    .expect("BRIOLETTE_BRIDGE_ADDRESS required");
                let operator_key_hex = std::env::var("BRIOLETTE_OPERATOR_KEY")
                    .expect("BRIOLETTE_OPERATOR_KEY required");

                let bridge_address: alloy::primitives::Address = bridge_addr_hex.parse()
                    .expect("invalid BRIOLETTE_BRIDGE_ADDRESS");
                let signer: alloy::signers::local::PrivateKeySigner = operator_key_hex.parse()
                    .expect("invalid BRIOLETTE_OPERATOR_KEY");

                let alloy_client = Arc::new(AlloyEthereumClient::new(
                    rpc_url,
                    bridge_address,
                    signer,
                ).expect("failed to create Ethereum client"));

                // Load keys from on-chain registry
                let registry = OnChainKeyRegistry::new(alloy_client.clone());
                let keys = registry.load_keys().await
                    .expect("failed to load keys from on-chain registry");

                if !keys.is_valid() {
                    error!("on-chain key registry is empty — register keys first");
                    std::process::exit(1);
                }

                // Spawn background key refresh task
                let shared_keys = Arc::new(RwLock::new(keys.clone()));
                let registry_source: Arc<dyn KeyRegistrySource> = Arc::new(registry);
                let refresh_keys = shared_keys.clone();
                tokio::spawn(async move {
                    briolette_bridge::key_registry::key_refresh_loop(
                        registry_source,
                        refresh_keys,
                        std::time::Duration::from_secs(60),
                    )
                    .await;
                });

                info!("using on-chain key registry, version={}", keys.version);
                (alloy_client as Arc<dyn briolette_bridge::ethereum::EthereumClient>, keys)
            }

            #[cfg(not(feature = "alloy"))]
            {
                panic!("onchain key source requires the `alloy` feature");
            }
        } else {
            // File-based keys (default / development mode)
            let file_registry = FileKeyRegistry::new(
                "../registrar/data/wallet.ttc.gpk".to_string(),
                vec!["../mint/data/mint.pk".to_string()],
                vec!["../clerk/data/ticket.pk".to_string()],
            );

            let keys = file_registry
                .load_keys()
                .await
                .expect("failed to load keys from files");

            let mock_client = Arc::new(MockEthereumClient::default());
            info!("using file-based keys with mock Ethereum client");
            (mock_client as Arc<dyn briolette_bridge::ethereum::EthereumClient>, keys)
        };

    // Create the bridge service
    let bridge = BrioletteBridge::new(
        eth_client.clone(),
        keys.ttc_group_public_key.clone(),
        keys.mint_signing_keys.clone(),
        keys.ticket_signing_keys.clone(),
        "http://127.0.0.1:50055".to_string(),
    );

    // Spawn the deposit processor as a background task
    let ticket_registry = Arc::new(RwLock::new(TicketRegistry::new()));
    let mint_uri =
        std::env::var("BRIOLETTE_MINT_URI").unwrap_or_else(|_| "http://127.0.0.1:50054".into());

    let deposit_config = DepositProcessorConfig {
        mint_uri,
        ..Default::default()
    };

    let mut deposit_processor =
        DepositProcessor::new(eth_client, ticket_registry, deposit_config);

    tokio::spawn(async move {
        deposit_processor.run().await;
    });

    info!("bridge server starting on {}", addr);
    tonic::transport::Server::builder()
        .add_service(BridgeServer::new(BridgeGrpc { inner: bridge }))
        .serve(addr)
        .await?;
    Ok(())
}
