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

//! Shared HTTP infrastructure for Briolette's A2A and x402 protocol support.
//!
//! Provides common types, base64 serialization helpers for protobuf bytes fields,
//! and authentication middleware bridging ECDAA credentials to HTTP auth schemes.

pub mod auth;
pub mod serde_proto;

use briolette_receiver::server::BrioletteReceiver;
use briolette_wallet::WalletData;
use std::sync::{Arc, RwLock};

/// Shared application state for axum HTTP handlers.
///
/// Both A2A and x402 routes share this state, which holds references to the
/// core Briolette receiver (for payment processing) and an optional wallet
/// (for client-side x402 payment construction).
#[derive(Clone)]
pub struct AppState {
    /// The payment receiver, shared with the gRPC server.
    pub receiver: Arc<BrioletteReceiver>,
    /// Optional wallet for x402 client-side payment construction.
    pub wallet: Option<Arc<RwLock<WalletData>>>,
}
