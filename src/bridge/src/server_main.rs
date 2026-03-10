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

use briolette_bridge::ethereum::MockEthereumClient;
use briolette_bridge::server::BrioletteBridge;
use briolette_proto::briolette::bridge::bridge_server::BridgeServer;
use briolette_proto::briolette::bridge::*;
use log::*;
use std::sync::Arc;
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

    // Load verification keys from the existing infrastructure.
    let ttc_gpk = std::fs::read("../registrar/data/wallet.ttc.gpk")
        .expect("registrar/data/wallet.ttc.gpk not populated yet");
    let mint_pk =
        std::fs::read("../mint/data/mint.pk").expect("mint/data/mint.pk not populated yet");
    let ticket_pk = std::fs::read("../clerk/data/ticket.pk")
        .expect("clerk/data/ticket.pk not populated yet");

    // Use mock Ethereum client for development.
    // Replace with a real implementation (ethers-rs/alloy) for production.
    let eth_client = Arc::new(MockEthereumClient::default());

    let bridge = BrioletteBridge::new(
        eth_client,
        ttc_gpk,
        vec![mint_pk],
        vec![ticket_pk],
        "http://127.0.0.1:50055".to_string(),
    );

    info!("bridge server starting on {}", addr);
    tonic::transport::Server::builder()
        .add_service(BridgeServer::new(BridgeGrpc { inner: bridge }))
        .serve(addr)
        .await?;
    Ok(())
}
