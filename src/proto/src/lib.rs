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

pub mod briolette;
pub mod rate_limit;

// TODO(redpig) Bump these out into a separate helper crate to keep the dependencies
// lighter.
use log::*;
use tokio::net::UnixStream;
use tonic::transport::{Endpoint, Uri};
use tower::service_fn;

// Helpers
pub mod vec_utils {
    pub fn vec_equal(l: &Vec<u8>, r: &Vec<u8>) -> bool {
        l.len() == r.len() && l.iter().zip(r).all(|(a, b)| *a == *b)
    }
}

/// TLS configuration helpers for servers and clients.
pub mod tls {
    use std::path::Path;
    use tonic::transport::{Certificate, Identity, ServerTlsConfig, ClientTlsConfig};

    /// Build a server TLS config from PEM files.
    pub fn server_tls_config(
        cert_path: &Path,
        key_path: &Path,
        ca_cert_path: Option<&Path>,
    ) -> Result<ServerTlsConfig, Box<dyn std::error::Error>> {
        let cert_pem = std::fs::read_to_string(cert_path)?;
        let key_pem = std::fs::read_to_string(key_path)?;
        let identity = Identity::from_pem(cert_pem, key_pem);

        let mut config = ServerTlsConfig::new().identity(identity);
        if let Some(ca_path) = ca_cert_path {
            let ca_pem = std::fs::read_to_string(ca_path)?;
            config = config.client_ca_root(Certificate::from_pem(ca_pem));
        }
        Ok(config)
    }

    /// Build a client TLS config from optional CA certificate.
    pub fn client_tls_config(
        ca_cert_path: Option<&Path>,
        domain: Option<&str>,
    ) -> Result<ClientTlsConfig, Box<dyn std::error::Error>> {
        let mut config = ClientTlsConfig::new();
        if let Some(domain) = domain {
            config = config.domain_name(domain);
        }
        if let Some(ca_path) = ca_cert_path {
            let ca_pem = std::fs::read_to_string(ca_path)?;
            config = config.ca_certificate(Certificate::from_pem(ca_pem));
        }
        Ok(config)
    }
}

#[tonic::async_trait]
pub trait BrioletteClientHelper: Sized {
    // Wraps the call to TonicClient::new(Channel)
    fn new_wrapper(channel: tonic::transport::Channel) -> Self;

    // Add support for the socket://localhost URI scheme and authority which
    // enables easy switching between UNIX domain sockets and TCP.
    async fn multiconnect(uri: &Uri) -> Result<Box<Self>, tonic::transport::Error> {
        Self::multiconnect_tls(uri, None).await
    }

    /// Connect with optional TLS configuration.
    async fn multiconnect_tls(
        uri: &Uri,
        tls_config: Option<tonic::transport::ClientTlsConfig>,
    ) -> Result<Box<Self>, tonic::transport::Error> {
        let channel = match uri.scheme_str() {
            Some("socket") => {
                Endpoint::from(uri.clone())
                    .connect_with_connector(service_fn(|uri: Uri| {
                        info!("Connecting to socket at {:?}", uri);
                        // N.b., format!() is used to extract path() without creating a local reference in this
                        //       function, which will then go out of scope.
                        // TODO(redpig) Send pull request updating uds example in tonic.
                        UnixStream::connect(format!("{}", uri.path()))
                    }))
                    .await
            }
            _ => {
                let mut endpoint = Endpoint::from(uri.clone());
                if let Some(tls) = tls_config {
                    endpoint = endpoint.tls_config(tls)?;
                }
                endpoint.connect().await
            }
        };
        if channel.is_ok() {
            trace!("Client channel connection established");
            Ok(Box::new(Self::new_wrapper(channel.unwrap())))
        } else {
            Err(channel.err().unwrap())
        }
    }
}
