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

//! Maps A2A task lifecycle to Briolette's Receiver protocol.
//!
//! The A2A task lifecycle for a payment flows as:
//!
//! 1. `tasks/send` (new task) → `initiate_impl()` → status: `input-required`
//!    Artifact: payment items (amount, description, recipient ticket)
//!
//! 2. `tasks/send` (with token proposal) → `transact_impl()` → status: `working`
//!    The agent proposes tokens to settle the payment.
//!
//! 3. `tasks/send` (with signed transfer) → `transfer_impl()` → status: `completed`
//!    The agent sends the final signed token transfer.
//!
//! 4. `tasks/get` → reads current task state
//! 5. `tasks/cancel` → removes task

use crate::types::*;
use briolette_http_common::AppState;
use briolette_proto::briolette::receiver::*;
use briolette_proto::briolette::token;
use log::*;
use prost::Message as ProstMessage;
use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::{Arc, RwLock};
use uuid::Uuid;

/// Manages A2A tasks backed by the Briolette Receiver.
#[derive(Clone)]
pub struct TaskManager {
    state: AppState,
    /// Maps A2A task IDs to their current state and the Receiver's tx_id.
    tasks: Arc<RwLock<HashMap<String, TaskRecord>>>,
}

/// Internal tracking for an A2A task.
#[derive(Debug, Clone)]
struct TaskRecord {
    task: Task,
    /// The Receiver protocol's transaction ID (tx_id bytes).
    tx_id: Option<Vec<u8>>,
    /// The payment items returned by Initiate.
    items: Vec<TransactionItem>,
    /// The recipient ticket for this transaction.
    ticket: Option<token::SignedTicket>,
}

impl TaskManager {
    pub fn new(state: AppState) -> Self {
        Self {
            state,
            tasks: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Handle `tasks/send` — create a new task or continue an existing one.
    pub async fn send(
        &self,
        params: TaskSendParams,
        peer: Option<SocketAddr>,
    ) -> Result<Task, JsonRpcError> {
        match params.id {
            None => self.create_task(params.message, peer).await,
            Some(ref id) => self.continue_task(id, params.message, peer).await,
        }
    }

    /// Handle `tasks/get` — retrieve task status.
    pub fn get(&self, params: TaskGetParams) -> Result<Task, JsonRpcError> {
        let tasks = self.tasks.read().unwrap();
        let record = tasks.get(&params.id).ok_or_else(|| JsonRpcError {
            code: TASK_NOT_FOUND,
            message: format!("Task {} not found", params.id),
            data: None,
        })?;

        let mut task = record.task.clone();
        // Optionally truncate history.
        if let Some(max) = params.history_length {
            let max = max as usize;
            if task.history.len() > max {
                let start = task.history.len() - max;
                task.history = task.history[start..].to_vec();
            }
        }
        Ok(task)
    }

    /// Handle `tasks/cancel` — cancel a task if it's not completed.
    pub fn cancel(&self, params: TaskCancelParams) -> Result<Task, JsonRpcError> {
        let mut tasks = self.tasks.write().unwrap();
        let record = tasks.get_mut(&params.id).ok_or_else(|| JsonRpcError {
            code: TASK_NOT_FOUND,
            message: format!("Task {} not found", params.id),
            data: None,
        })?;

        match record.task.status.state {
            TaskState::Completed | TaskState::Failed | TaskState::Canceled => {
                return Err(JsonRpcError {
                    code: TASK_NOT_CANCELABLE,
                    message: "Task is already in a terminal state".to_string(),
                    data: None,
                });
            }
            _ => {}
        }

        record.task.status = TaskStatus {
            state: TaskState::Canceled,
            message: Some(Message {
                role: MessageRole::Agent,
                parts: vec![Part::Text {
                    text: "Payment canceled".to_string(),
                }],
                metadata: None,
            }),
        };

        Ok(record.task.clone())
    }

    /// Create a new payment task via Receiver::initiate_impl().
    async fn create_task(
        &self,
        message: Message,
        peer: Option<SocketAddr>,
    ) -> Result<Task, JsonRpcError> {
        let task_id = Uuid::new_v4().to_string();

        // Call the Receiver's initiate_impl.
        let request = InitiateRequest::default();
        let reply = self
            .state
            .receiver
            .initiate_impl(&request, peer)
            .await
            .map_err(|e| JsonRpcError {
                code: INTERNAL_ERROR,
                message: format!("Initiation failed: {:?}", e),
                data: None,
            })?;

        // Build the response artifact with payment details.
        let payment_details = serde_json::json!({
            "tx_id": reply.tx_id,
            "items": reply.items.iter().map(|item| {
                serde_json::json!({
                    "name": item.name,
                    "description": item.description,
                    "amount": item.amount.as_ref().map(|a| {
                        serde_json::json!({
                            "whole": a.whole,
                            "fractional": a.fractional,
                            "code": a.code,
                        })
                    }),
                })
            }).collect::<Vec<_>>(),
        });

        let task = Task {
            id: task_id.clone(),
            status: TaskStatus {
                state: TaskState::InputRequired,
                message: Some(Message {
                    role: MessageRole::Agent,
                    parts: vec![Part::Text {
                        text: "Payment session initiated. Please provide tokens to settle."
                            .to_string(),
                    }],
                    metadata: None,
                }),
            },
            artifacts: vec![Artifact {
                name: Some("payment-request".to_string()),
                description: Some("Payment details and recipient ticket".to_string()),
                parts: vec![Part::Data {
                    data: payment_details,
                    metadata: None,
                }],
                index: 0,
                metadata: None,
            }],
            history: vec![message],
            metadata: None,
        };

        let record = TaskRecord {
            task: task.clone(),
            tx_id: Some(reply.tx_id),
            items: reply.items,
            ticket: reply.ticket,
        };

        self.tasks.write().unwrap().insert(task_id, record);

        Ok(task)
    }

    /// Continue an existing task based on its current state.
    async fn continue_task(
        &self,
        task_id: &str,
        message: Message,
        peer: Option<SocketAddr>,
    ) -> Result<Task, JsonRpcError> {
        let current_state = {
            let tasks = self.tasks.read().unwrap();
            let record = tasks.get(task_id).ok_or_else(|| JsonRpcError {
                code: TASK_NOT_FOUND,
                message: format!("Task {} not found", task_id),
                data: None,
            })?;
            record.task.status.state.clone()
        };

        match current_state {
            TaskState::InputRequired => {
                // Agent is providing token proposal → transact_impl()
                self.handle_transact(task_id, message, peer).await
            }
            TaskState::Working => {
                // Agent is providing signed transfer → transfer_impl()
                self.handle_transfer(task_id, message, peer).await
            }
            TaskState::Completed | TaskState::Failed | TaskState::Canceled => {
                Err(JsonRpcError {
                    code: INVALID_PARAMS,
                    message: "Task is already in a terminal state".to_string(),
                    data: None,
                })
            }
            _ => Err(JsonRpcError {
                code: INVALID_PARAMS,
                message: format!("Unexpected task state: {:?}", current_state),
                data: None,
            }),
        }
    }

    /// Handle token proposal via transact_impl().
    async fn handle_transact(
        &self,
        task_id: &str,
        message: Message,
        peer: Option<SocketAddr>,
    ) -> Result<Task, JsonRpcError> {
        // Extract tokens from the message's data parts.
        let tokens = self.extract_tokens(&message)?;

        let tx_id = {
            let tasks = self.tasks.read().unwrap();
            let record = tasks.get(task_id).unwrap();
            record.tx_id.clone().unwrap_or_default()
        };

        let request = TransactRequest {
            tx_id,
            methods: vec![TransactionItemMethod {
                tokens,
                mint_public_key: vec![],
            }],
        };

        let reply = self
            .state
            .receiver
            .transact_impl(&request, peer)
            .await
            .map_err(|e| JsonRpcError {
                code: INTERNAL_ERROR,
                message: format!("Transaction proposal failed: {:?}", e),
                data: None,
            })?;

        let mut tasks = self.tasks.write().unwrap();
        let record = tasks.get_mut(task_id).unwrap();

        if reply.accept {
            record.task.status = TaskStatus {
                state: TaskState::Working,
                message: Some(Message {
                    role: MessageRole::Agent,
                    parts: vec![Part::Text {
                        text: "Token proposal accepted. Please send the signed transfer."
                            .to_string(),
                    }],
                    metadata: None,
                }),
            };
        } else {
            record.task.status = TaskStatus {
                state: TaskState::Failed,
                message: Some(Message {
                    role: MessageRole::Agent,
                    parts: vec![Part::Text {
                        text: "Token proposal rejected — invalid tokens or incorrect amount."
                            .to_string(),
                    }],
                    metadata: None,
                }),
            };
        }
        record.task.history.push(message);

        Ok(record.task.clone())
    }

    /// Handle signed transfer via transfer_impl().
    async fn handle_transfer(
        &self,
        task_id: &str,
        message: Message,
        peer: Option<SocketAddr>,
    ) -> Result<Task, JsonRpcError> {
        let tokens = self.extract_tokens(&message)?;

        let tx_id = {
            let tasks = self.tasks.read().unwrap();
            let record = tasks.get(task_id).unwrap();
            record.tx_id.clone().unwrap_or_default()
        };

        let request = TransferRequest {
            tx_id,
            tokens,
        };

        let reply = self
            .state
            .receiver
            .transfer_impl(&request, peer)
            .await
            .map_err(|e| JsonRpcError {
                code: INTERNAL_ERROR,
                message: format!("Transfer failed: {:?}", e),
                data: None,
            })?;

        let mut tasks = self.tasks.write().unwrap();
        let record = tasks.get_mut(task_id).unwrap();

        if reply.accepted {
            record.task.status = TaskStatus {
                state: TaskState::Completed,
                message: Some(Message {
                    role: MessageRole::Agent,
                    parts: vec![Part::Text {
                        text: "Payment completed successfully.".to_string(),
                    }],
                    metadata: None,
                }),
            };
            record.task.artifacts.push(Artifact {
                name: Some("payment-receipt".to_string()),
                description: Some("Confirmation of payment".to_string()),
                parts: vec![Part::Data {
                    data: serde_json::json!({
                        "accepted": true,
                        "task_id": task_id,
                    }),
                    metadata: None,
                }],
                index: 1,
                metadata: None,
            });
        } else {
            record.task.status = TaskStatus {
                state: TaskState::Failed,
                message: Some(Message {
                    role: MessageRole::Agent,
                    parts: vec![Part::Text {
                        text: "Transfer rejected.".to_string(),
                    }],
                    metadata: None,
                }),
            };
        }
        record.task.history.push(message);

        Ok(record.task.clone())
    }

    /// Extract Token protos from the data parts of a message.
    fn extract_tokens(&self, message: &Message) -> Result<Vec<token::Token>, JsonRpcError> {
        for part in &message.parts {
            if let Part::Data { data, .. } = part {
                // Expect data to contain a "tokens" array of protobuf-encoded tokens (base64).
                if let Some(tokens_arr) = data.get("tokens").and_then(|v| v.as_array()) {
                    let mut tokens = Vec::new();
                    for token_val in tokens_arr {
                        // Try to deserialize from the serde-derived JSON representation.
                        let token: token::Token =
                            serde_json::from_value(token_val.clone()).map_err(|e| {
                                JsonRpcError {
                                    code: INVALID_PARAMS,
                                    message: format!("Failed to parse token: {}", e),
                                    data: None,
                                }
                            })?;
                        tokens.push(token);
                    }
                    return Ok(tokens);
                }
            }
        }
        Err(JsonRpcError {
            code: INVALID_PARAMS,
            message: "Message must contain a data part with a 'tokens' array".to_string(),
            data: None,
        })
    }
}
