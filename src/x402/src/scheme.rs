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

//! Briolette payment scheme definition for x402.

/// The x402 scheme identifier for Briolette payments.
pub const SCHEME_NAME: &str = "briolette";

/// Current version of the Briolette x402 scheme.
pub const SCHEME_VERSION: &str = "0.1.0";

/// HTTP header name for the payment payload (request).
pub const PAYMENT_HEADER: &str = "X-PAYMENT";

/// HTTP header name for the payment receipt (response).
pub const PAYMENT_RECEIPT_HEADER: &str = "X-PAYMENT-RECEIPT";

/// HTTP header name for payment requirements (402 response).
pub const PAYMENT_REQUIREMENTS_HEADER: &str = "X-PAYMENT-REQUIREMENTS";
