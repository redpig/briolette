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

//! x402 axum middleware — returns HTTP 402 for paywalled resources.
//!
//! Flow:
//! 1. Request arrives without `X-PAYMENT` header → return 402 with requirements
//! 2. Request arrives with `X-PAYMENT` header → verify payment, forward if valid
//!
//! Usage:
//! ```ignore
//! let paywall = PaywallLayer::new(facilitator, requirements);
//! let app = Router::new()
//!     .route("/paid-resource", get(handler))
//!     .layer(paywall);
//! ```

use crate::facilitator::Facilitator;
use crate::scheme;
use crate::types::{PaymentPayload, PaymentRequirements};
use axum::{
    body::{self, BoxBody, Full},
    http::{header, Request, Response, StatusCode},
};
use briolette_proto::briolette::token;
use futures_util::future::BoxFuture;
use log::*;
use std::sync::Arc;
use std::task::{Context, Poll};
use tower::{Layer, Service};

/// Configuration for a paywalled endpoint.
#[derive(Clone)]
pub struct PaywallConfig {
    pub requirements: PaymentRequirements,
    pub required_amount: token::Amount,
}

/// Tower Layer that wraps services with x402 payment verification.
#[derive(Clone)]
pub struct PaywallLayer {
    facilitator: Arc<Facilitator>,
    config: PaywallConfig,
}

impl PaywallLayer {
    pub fn new(facilitator: Facilitator, config: PaywallConfig) -> Self {
        Self {
            facilitator: Arc::new(facilitator),
            config,
        }
    }
}

impl<S> Layer<S> for PaywallLayer {
    type Service = PaywallMiddleware<S>;

    fn layer(&self, inner: S) -> Self::Service {
        PaywallMiddleware {
            inner,
            facilitator: self.facilitator.clone(),
            config: self.config.clone(),
        }
    }
}

/// The middleware service that checks for payment before forwarding requests.
#[derive(Clone)]
pub struct PaywallMiddleware<S> {
    inner: S,
    facilitator: Arc<Facilitator>,
    config: PaywallConfig,
}

impl<S, ReqBody> Service<Request<ReqBody>> for PaywallMiddleware<S>
where
    S: Service<Request<ReqBody>, Response = Response<BoxBody>> + Clone + Send + 'static,
    S::Future: Send + 'static,
    ReqBody: Send + 'static,
{
    type Response = Response<BoxBody>;
    type Error = S::Error;
    type Future = BoxFuture<'static, Result<Self::Response, Self::Error>>;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx)
    }

    fn call(&mut self, req: Request<ReqBody>) -> Self::Future {
        let facilitator = self.facilitator.clone();
        let config = self.config.clone();
        let mut inner = self.inner.clone();

        Box::pin(async move {
            // Check for payment header.
            let payment_header = req
                .headers()
                .get(scheme::PAYMENT_HEADER)
                .and_then(|v| v.to_str().ok())
                .map(|s| s.to_string());

            match payment_header {
                None => {
                    // No payment → return 402 with requirements.
                    debug!("No payment header, returning 402");
                    let requirements_json =
                        serde_json::to_string(&config.requirements).unwrap_or_default();

                    let response = Response::builder()
                        .status(StatusCode::PAYMENT_REQUIRED)
                        .header(header::CONTENT_TYPE, "application/json")
                        .header(scheme::PAYMENT_REQUIREMENTS_HEADER, &requirements_json)
                        .body(body::boxed(Full::from(requirements_json)))
                        .unwrap();

                    Ok(response)
                }
                Some(payment_str) => {
                    // Parse and verify payment.
                    let payload: PaymentPayload = match serde_json::from_str(&payment_str) {
                        Ok(p) => p,
                        Err(e) => {
                            warn!("Failed to parse payment payload: {}", e);
                            let response = Response::builder()
                                .status(StatusCode::BAD_REQUEST)
                                .body(body::boxed(Full::from(format!(
                                    "Invalid payment payload: {}",
                                    e
                                ))))
                                .unwrap();
                            return Ok(response);
                        }
                    };

                    if payload.scheme != scheme::SCHEME_NAME {
                        let response = Response::builder()
                            .status(StatusCode::BAD_REQUEST)
                            .body(body::boxed(Full::from(format!(
                                "Unsupported payment scheme: {}",
                                payload.scheme
                            ))))
                            .unwrap();
                        return Ok(response);
                    }

                    // Deserialize tokens from the payload.
                    let tokens: Vec<token::Token> = match payload
                        .payload
                        .tokens
                        .iter()
                        .map(|v| serde_json::from_value(v.clone()))
                        .collect::<Result<Vec<_>, _>>()
                    {
                        Ok(t) => t,
                        Err(e) => {
                            warn!("Failed to deserialize tokens: {}", e);
                            let response = Response::builder()
                                .status(StatusCode::BAD_REQUEST)
                                .body(body::boxed(Full::from(format!(
                                    "Invalid token data: {}",
                                    e
                                ))))
                                .unwrap();
                            return Ok(response);
                        }
                    };

                    // Verify the payment.
                    match facilitator.verify_payment(
                        &tokens,
                        &config.required_amount,
                        &config.requirements.resource,
                        &config.requirements.network,
                    ) {
                        Ok(receipt) => {
                            info!("Payment verified, forwarding request");
                            // Forward to inner service, adding receipt header.
                            let mut response = inner.call(req).await?;
                            let receipt_json =
                                serde_json::to_string(&receipt).unwrap_or_default();
                            response.headers_mut().insert(
                                scheme::PAYMENT_RECEIPT_HEADER,
                                receipt_json.parse().unwrap(),
                            );
                            Ok(response)
                        }
                        Err(e) => {
                            warn!("Payment verification failed: {}", e);
                            let response = Response::builder()
                                .status(StatusCode::PAYMENT_REQUIRED)
                                .body(body::boxed(Full::from(format!(
                                    "Payment verification failed: {}",
                                    e
                                ))))
                                .unwrap();
                            Ok(response)
                        }
                    }
                }
            }
        })
    }
}
