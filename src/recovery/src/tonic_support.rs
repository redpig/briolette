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

use crate::server::BrioletteRecovery;
use briolette_proto::briolette::recovery::recovery_server::Recovery;
use briolette_proto::briolette::recovery::{
    GetBindingStatusReply, GetBindingStatusRequest, RecoverTokensReply, RecoverTokensRequest,
    RefreshBindingReply, RefreshBindingRequest, RegisterBindingReply, RegisterBindingRequest,
    RevokeBindingReply, RevokeBindingRequest,
};
use tonic::{Request, Response, Status};

#[tonic::async_trait]
impl Recovery for BrioletteRecovery {
    async fn register_binding(
        &self,
        request: Request<RegisterBindingRequest>,
    ) -> Result<Response<RegisterBindingReply>, Status> {
        let message = request.into_inner();
        let maybe_reply = self.register_binding_impl(&message).await;
        match maybe_reply {
            Ok(reply) => Ok(Response::new(reply)),
            Err(status) => Err(status.into()),
        }
    }

    async fn refresh_binding(
        &self,
        request: Request<RefreshBindingRequest>,
    ) -> Result<Response<RefreshBindingReply>, Status> {
        let message = request.into_inner();
        let maybe_reply = self.refresh_binding_impl(&message).await;
        match maybe_reply {
            Ok(reply) => Ok(Response::new(reply)),
            Err(status) => Err(status.into()),
        }
    }

    async fn revoke_binding(
        &self,
        request: Request<RevokeBindingRequest>,
    ) -> Result<Response<RevokeBindingReply>, Status> {
        let message = request.into_inner();
        let maybe_reply = self.revoke_binding_impl(&message).await;
        match maybe_reply {
            Ok(reply) => Ok(Response::new(reply)),
            Err(status) => Err(status.into()),
        }
    }

    async fn recover_tokens(
        &self,
        request: Request<RecoverTokensRequest>,
    ) -> Result<Response<RecoverTokensReply>, Status> {
        let message = request.into_inner();
        let maybe_reply = self.recover_tokens_impl(&message).await;
        match maybe_reply {
            Ok(reply) => Ok(Response::new(reply)),
            Err(status) => Err(status.into()),
        }
    }

    async fn get_binding_status(
        &self,
        request: Request<GetBindingStatusRequest>,
    ) -> Result<Response<GetBindingStatusReply>, Status> {
        let message = request.into_inner();
        let maybe_reply = self.get_binding_status_impl(&message).await;
        match maybe_reply {
            Ok(reply) => Ok(Response::new(reply)),
            Err(status) => Err(status.into()),
        }
    }
}
