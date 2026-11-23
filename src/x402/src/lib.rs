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

//! x402 HTTP payment protocol support for Briolette.
//!
//! Implements the x402 protocol (HTTP 402 Payment Required) to enable
//! pay-per-request APIs using Briolette tokens. Defines the "briolette"
//! payment scheme and provides server middleware and client logic.

pub mod client;
pub mod facilitator;
pub mod middleware;
pub mod scheme;
pub mod types;
