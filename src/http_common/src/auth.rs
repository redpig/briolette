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

//! Authentication middleware bridging HTTP auth schemes to Briolette's
//! ECDAA credential system.
//!
//! For agent-facing HTTP endpoints (A2A, x402), this module provides:
//! - Bearer token validation (for API key or OAuth2 token auth)
//! - Extraction of caller identity for peer tracking
//!
//! The actual ECDAA credential verification for token operations happens
//! in the wallet/receiver layer, not here. This middleware handles the
//! HTTP-level auth that lets agents access the A2A/x402 endpoints.

use axum::{
    extract::FromRequestParts,
    http::{request::Parts, StatusCode},
};

/// Extracted authentication identity from an HTTP request.
///
/// This is intentionally simple for now — it extracts a Bearer token
/// from the Authorization header. Production deployments would extend
/// this to validate tokens against an identity provider or map them
/// to ECDAA credentials.
#[derive(Debug, Clone)]
pub struct AuthIdentity {
    /// The bearer token or API key, if present.
    pub token: Option<String>,
}

#[axum::async_trait]
impl<S> FromRequestParts<S> for AuthIdentity
where
    S: Send + Sync,
{
    type Rejection = (StatusCode, &'static str);

    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, Self::Rejection> {
        let token = parts
            .headers
            .get("authorization")
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.strip_prefix("Bearer "))
            .map(|s| s.to_string());

        Ok(AuthIdentity { token })
    }
}

/// Require authentication — rejects requests without a Bearer token.
///
/// Use as an axum extractor in handlers that require authentication:
/// ```ignore
/// async fn protected_handler(RequireAuth(identity): RequireAuth) -> impl IntoResponse {
///     // identity.token is guaranteed to be Some
/// }
/// ```
#[derive(Debug, Clone)]
pub struct RequireAuth(pub AuthIdentity);

#[axum::async_trait]
impl<S> FromRequestParts<S> for RequireAuth
where
    S: Send + Sync,
{
    type Rejection = (StatusCode, &'static str);

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        let identity = AuthIdentity::from_request_parts(parts, state).await?;
        if identity.token.is_none() {
            return Err((StatusCode::UNAUTHORIZED, "Bearer token required"));
        }
        Ok(RequireAuth(identity))
    }
}
