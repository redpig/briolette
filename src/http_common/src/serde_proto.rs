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

//! Base64 serialization helpers for protobuf bytes fields in JSON contexts.
//!
//! Protobuf `bytes` fields are generated as `Vec<u8>` by prost. When serialized
//! to JSON via serde, they become arrays of numbers `[104, 101, 108, ...]`.
//! These helpers provide base64 encoding/decoding for cleaner JSON representation
//! in the A2A and x402 HTTP APIs.

use base64::{engine::general_purpose::STANDARD, Engine};
use serde::{Deserialize, Deserializer, Serialize, Serializer};

/// Serialize `Vec<u8>` as a base64 string.
pub fn serialize_bytes<S>(bytes: &Vec<u8>, serializer: S) -> Result<S::Ok, S::Error>
where
    S: Serializer,
{
    STANDARD.encode(bytes).serialize(serializer)
}

/// Deserialize a base64 string into `Vec<u8>`.
pub fn deserialize_bytes<'de, D>(deserializer: D) -> Result<Vec<u8>, D::Error>
where
    D: Deserializer<'de>,
{
    let s = String::deserialize(deserializer)?;
    STANDARD.decode(&s).map_err(serde::de::Error::custom)
}

/// A wrapper around `Vec<u8>` that serializes as base64 in JSON.
///
/// Use this when you need a self-contained base64-serialized bytes field
/// in a JSON-facing struct (e.g., x402 PaymentPayload, A2A artifacts).
#[derive(Debug, Clone, PartialEq)]
pub struct Base64Bytes(pub Vec<u8>);

impl Serialize for Base64Bytes {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        STANDARD.encode(&self.0).serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for Base64Bytes {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        STANDARD
            .decode(&s)
            .map(Base64Bytes)
            .map_err(serde::de::Error::custom)
    }
}

impl From<Vec<u8>> for Base64Bytes {
    fn from(v: Vec<u8>) -> Self {
        Base64Bytes(v)
    }
}

impl From<Base64Bytes> for Vec<u8> {
    fn from(b: Base64Bytes) -> Self {
        b.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base64_bytes_round_trip() {
        let original = vec![0xDE, 0xAD, 0xBE, 0xEF];
        let b64 = Base64Bytes(original.clone());
        let json = serde_json::to_string(&b64).unwrap();
        assert_eq!(json, "\"3q2+7w==\"");
        let decoded: Base64Bytes = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded.0, original);
    }

    #[test]
    fn empty_bytes_round_trip() {
        let b64 = Base64Bytes(vec![]);
        let json = serde_json::to_string(&b64).unwrap();
        assert_eq!(json, "\"\"");
        let decoded: Base64Bytes = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded.0, Vec::<u8>::new());
    }
}
