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

//! A2A (Agent-to-Agent) protocol support for Briolette.
//!
//! Implements Google's A2A protocol to expose Briolette's payment receiver
//! as an AI agent-discoverable service. Agents can discover the payment
//! capability via Agent Cards and execute payment tasks via JSON-RPC 2.0.

pub mod routes;
pub mod task_manager;
pub mod types;
