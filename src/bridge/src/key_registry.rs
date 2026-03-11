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

//! On-chain key management for the Briolette bridge.
//!
//! `KeyRegistrySource` defines how system keys (mint, ticket, TTC group) are
//! loaded. This abstraction lets the bridge server load keys from either:
//! - Local files (existing behavior, for development)
//! - The on-chain BrioletteBridge contract registry (production)
//!
//! The `OnChainKeyRegistry` polls the contract for key updates and caches
//! them locally, refreshing when the on-chain version changes.

use async_trait::async_trait;
use log::*;
use std::sync::Arc;
use tokio::sync::RwLock;

/// Cached key material from the registry.
#[derive(Debug, Clone, Default)]
pub struct KeyMaterial {
    pub ttc_group_public_key: Vec<u8>,
    pub mint_signing_keys: Vec<Vec<u8>>,
    pub ticket_signing_keys: Vec<Vec<u8>>,
    /// Registry version for change detection.
    pub version: u64,
}

impl KeyMaterial {
    /// Check if the key material is populated (non-empty).
    pub fn is_valid(&self) -> bool {
        !self.ttc_group_public_key.is_empty()
            && !self.mint_signing_keys.is_empty()
            && !self.ticket_signing_keys.is_empty()
    }
}

/// Trait for loading key material from different sources.
#[async_trait]
pub trait KeyRegistrySource: Send + Sync {
    /// Load current key material. Returns None if keys aren't available yet.
    async fn load_keys(&self) -> Result<KeyMaterial, String>;

    /// Check if the key material has changed since the given version.
    async fn has_changed(&self, since_version: u64) -> Result<bool, String>;
}

/// File-based key registry (existing behavior for development).
pub struct FileKeyRegistry {
    ttc_gpk_path: String,
    mint_pk_paths: Vec<String>,
    ticket_pk_paths: Vec<String>,
}

impl FileKeyRegistry {
    pub fn new(
        ttc_gpk_path: String,
        mint_pk_paths: Vec<String>,
        ticket_pk_paths: Vec<String>,
    ) -> Self {
        Self {
            ttc_gpk_path,
            mint_pk_paths,
            ticket_pk_paths,
        }
    }
}

#[async_trait]
impl KeyRegistrySource for FileKeyRegistry {
    async fn load_keys(&self) -> Result<KeyMaterial, String> {
        let ttc_gpk =
            std::fs::read(&self.ttc_gpk_path).map_err(|e| format!("read ttc_gpk: {}", e))?;

        let mint_keys: Result<Vec<Vec<u8>>, String> = self
            .mint_pk_paths
            .iter()
            .map(|p| std::fs::read(p).map_err(|e| format!("read mint_pk {}: {}", p, e)))
            .collect();

        let ticket_keys: Result<Vec<Vec<u8>>, String> = self
            .ticket_pk_paths
            .iter()
            .map(|p| std::fs::read(p).map_err(|e| format!("read ticket_pk {}: {}", p, e)))
            .collect();

        Ok(KeyMaterial {
            ttc_group_public_key: ttc_gpk,
            mint_signing_keys: mint_keys?,
            ticket_signing_keys: ticket_keys?,
            version: 0, // File-based has no versioning
        })
    }

    async fn has_changed(&self, _since_version: u64) -> Result<bool, String> {
        // File-based registry doesn't support change detection.
        // Always returns false — keys are loaded once at startup.
        Ok(false)
    }
}

/// On-chain key registry backed by the BrioletteBridge contract.
///
/// Uses the `AlloyEthereumClient` to read keys from the contract's
/// key registry functions. Caches results and refreshes when the
/// on-chain version number changes.
#[cfg(feature = "alloy")]
pub struct OnChainKeyRegistry {
    eth_client: Arc<crate::alloy_client::AlloyEthereumClient>,
    cache: Arc<RwLock<KeyMaterial>>,
}

#[cfg(feature = "alloy")]
impl OnChainKeyRegistry {
    pub fn new(eth_client: Arc<crate::alloy_client::AlloyEthereumClient>) -> Self {
        Self {
            eth_client,
            cache: Arc::new(RwLock::new(KeyMaterial::default())),
        }
    }

    /// Get cached key material (for use in hot paths without async).
    pub async fn cached_keys(&self) -> KeyMaterial {
        self.cache.read().await.clone()
    }
}

#[cfg(feature = "alloy")]
#[async_trait]
impl KeyRegistrySource for OnChainKeyRegistry {
    async fn load_keys(&self) -> Result<KeyMaterial, String> {
        let ttc_gpk = self
            .eth_client
            .get_ttc_group_key()
            .await
            .map_err(|e| format!("fetch ttc_gpk: {}", e))?;

        let mint_keys = self
            .eth_client
            .get_mint_keys()
            .await
            .map_err(|e| format!("fetch mint_keys: {}", e))?;

        let ticket_keys = self
            .eth_client
            .get_ticket_keys()
            .await
            .map_err(|e| format!("fetch ticket_keys: {}", e))?;

        let version = self
            .eth_client
            .get_key_registry_version()
            .await
            .map_err(|e| format!("fetch version: {}", e))?;

        let material = KeyMaterial {
            ttc_group_public_key: ttc_gpk,
            mint_signing_keys: mint_keys,
            ticket_signing_keys: ticket_keys,
            version,
        };

        // Update cache
        *self.cache.write().await = material.clone();
        info!("key registry loaded from chain, version={}", version);

        Ok(material)
    }

    async fn has_changed(&self, since_version: u64) -> Result<bool, String> {
        let current_version = self
            .eth_client
            .get_key_registry_version()
            .await
            .map_err(|e| format!("check version: {}", e))?;

        Ok(current_version > since_version)
    }
}

/// Background task that periodically refreshes keys from the registry source.
pub async fn key_refresh_loop(
    source: Arc<dyn KeyRegistrySource>,
    keys: Arc<RwLock<KeyMaterial>>,
    poll_interval: std::time::Duration,
) {
    let mut interval = tokio::time::interval(poll_interval);
    let mut last_version = 0u64;

    loop {
        interval.tick().await;

        match source.has_changed(last_version).await {
            Ok(true) => {
                info!("key registry changed, refreshing...");
                match source.load_keys().await {
                    Ok(material) => {
                        last_version = material.version;
                        *keys.write().await = material;
                        info!("keys refreshed, version={}", last_version);
                    }
                    Err(e) => {
                        error!("failed to refresh keys: {}", e);
                    }
                }
            }
            Ok(false) => {
                trace!("key registry unchanged at version={}", last_version);
            }
            Err(e) => {
                warn!("failed to check key registry version: {}", e);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_key_material_validity() {
        let empty = KeyMaterial::default();
        assert!(!empty.is_valid());

        let valid = KeyMaterial {
            ttc_group_public_key: vec![1, 2, 3],
            mint_signing_keys: vec![vec![4, 5, 6]],
            ticket_signing_keys: vec![vec![7, 8, 9]],
            version: 1,
        };
        assert!(valid.is_valid());
    }

    #[tokio::test]
    async fn test_file_key_registry_has_changed() {
        let registry = FileKeyRegistry::new(
            "/nonexistent".to_string(),
            vec![],
            vec![],
        );
        // File registry never reports changes
        assert!(!registry.has_changed(0).await.unwrap());
    }
}
