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

//! A2A HTTP routes for axum.
//!
//! Provides:
//! - `GET /.well-known/agent.json` — Agent Card discovery
//! - `POST /a2a` — JSON-RPC 2.0 endpoint for task operations

use crate::task_manager::TaskManager;
use crate::types::*;
use axum::{
    extract::{ConnectInfo, State},
    http::StatusCode,
    response::IntoResponse,
    routing::{get, post},
    Json, Router,
};
use briolette_http_common::AppState;
use std::net::SocketAddr;

/// State for A2A routes, wrapping the shared AppState and TaskManager.
#[derive(Clone)]
pub struct A2aState {
    pub task_manager: TaskManager,
    pub base_url: String,
}

/// Build the A2A axum router.
pub fn router(app_state: AppState, base_url: String) -> Router {
    let a2a_state = A2aState {
        task_manager: TaskManager::new(app_state),
        base_url,
    };
    Router::new()
        .route("/.well-known/agent.json", get(agent_card_handler))
        .route("/a2a", post(jsonrpc_handler))
        .with_state(a2a_state)
}

/// Serve the Agent Card at `/.well-known/agent.json`.
async fn agent_card_handler(State(state): State<A2aState>) -> impl IntoResponse {
    let card = briolette_agent_card(&state.base_url);
    Json(card)
}

/// Handle JSON-RPC 2.0 requests on `/a2a`.
async fn jsonrpc_handler(
    State(state): State<A2aState>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    Json(request): Json<JsonRpcRequest>,
) -> impl IntoResponse {
    if request.jsonrpc != "2.0" {
        return (
            StatusCode::OK,
            Json(JsonRpcResponse::error(
                request.id,
                INVALID_REQUEST,
                "Expected jsonrpc version 2.0",
            )),
        );
    }

    let response = match request.method.as_str() {
        "tasks/send" => handle_tasks_send(&state, request.id.clone(), request.params, Some(addr)).await,
        "tasks/get" => handle_tasks_get(&state, request.id.clone(), request.params),
        "tasks/cancel" => handle_tasks_cancel(&state, request.id.clone(), request.params),
        _ => JsonRpcResponse::error(
            request.id,
            METHOD_NOT_FOUND,
            format!("Unknown method: {}", request.method),
        ),
    };

    (StatusCode::OK, Json(response))
}

async fn handle_tasks_send(
    state: &A2aState,
    id: serde_json::Value,
    params: serde_json::Value,
    peer: Option<SocketAddr>,
) -> JsonRpcResponse {
    let send_params: TaskSendParams = match serde_json::from_value(params) {
        Ok(p) => p,
        Err(e) => {
            return JsonRpcResponse::error(id, INVALID_PARAMS, format!("Invalid params: {}", e))
        }
    };

    match state.task_manager.send(send_params, peer).await {
        Ok(task) => JsonRpcResponse::success(id, serde_json::to_value(task).unwrap()),
        Err(e) => JsonRpcResponse {
            jsonrpc: "2.0".to_string(),
            id,
            result: None,
            error: Some(e),
        },
    }
}

fn handle_tasks_get(
    state: &A2aState,
    id: serde_json::Value,
    params: serde_json::Value,
) -> JsonRpcResponse {
    let get_params: TaskGetParams = match serde_json::from_value(params) {
        Ok(p) => p,
        Err(e) => {
            return JsonRpcResponse::error(id, INVALID_PARAMS, format!("Invalid params: {}", e))
        }
    };

    match state.task_manager.get(get_params) {
        Ok(task) => JsonRpcResponse::success(id, serde_json::to_value(task).unwrap()),
        Err(e) => JsonRpcResponse {
            jsonrpc: "2.0".to_string(),
            id,
            result: None,
            error: Some(e),
        },
    }
}

fn handle_tasks_cancel(
    state: &A2aState,
    id: serde_json::Value,
    params: serde_json::Value,
) -> JsonRpcResponse {
    let cancel_params: TaskCancelParams = match serde_json::from_value(params) {
        Ok(p) => p,
        Err(e) => {
            return JsonRpcResponse::error(id, INVALID_PARAMS, format!("Invalid params: {}", e))
        }
    };

    match state.task_manager.cancel(cancel_params) {
        Ok(task) => JsonRpcResponse::success(id, serde_json::to_value(task).unwrap()),
        Err(e) => JsonRpcResponse {
            jsonrpc: "2.0".to_string(),
            id,
            result: None,
            error: Some(e),
        },
    }
}
