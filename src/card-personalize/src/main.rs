// Copyright 2024 The Briolette Authors.
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

//! Card personalization CLI tool for Briolette manufacturer attestation.
//!
//! This tool simulates a card manufacturer's personalization station:
//! 1. Generates a P-256 ECDSA CA keypair (the "manufacturer key")
//! 2. Connects to a JavaCard via NFC (requires `nfc` feature)
//! 3. Triggers on-card P-256 key generation (MFR_GENERATE_KEY)
//! 4. Signs the card's public key with the CA key (creating a "certificate")
//! 5. Loads the certificate back onto the card (MFR_SET_CERT)
//! 6. Optionally tests the card's attestation (MFR_ATTEST)
//!
//! Build with NFC support: cargo build -p briolette-card-personalize --features nfc

use clap::{Parser, Subcommand};
use ecdsa::signature::Signer;
use p256::ecdsa::{DerSignature, SigningKey, VerifyingKey};
use p256::ecdsa::signature::Verifier;
use sha2::{Digest, Sha256};
use std::path::PathBuf;

/// Briolette card personalization tool.
///
/// Simulates a card manufacturer's personalization station for development
/// cards. Generates a P-256 CA keypair and uses it to certify cards.
#[derive(Parser)]
#[command(name = "briolette-card-personalize")]
#[command(about = "Card personalization tool for Briolette manufacturer attestation")]
struct Cli {
    /// Verbosity level (-v, -vv, -vvv)
    #[arg(short, long, action = clap::ArgAction::Count)]
    verbose: u8,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Generate a new manufacturer CA keypair
    GenerateCa {
        /// Directory to store the CA keypair files
        #[arg(short, long, default_value = ".")]
        output_dir: PathBuf,
    },
    /// Sign a card's public key offline (without NFC)
    SignKey {
        /// Path to the CA private key file (PEM)
        #[arg(short = 'k', long)]
        ca_key: PathBuf,
        /// Card public key as hex (65-byte uncompressed SEC1 point)
        #[arg(short = 'p', long)]
        card_pubkey_hex: String,
    },
    /// Verify an attestation response offline (without NFC)
    VerifyAttestation {
        /// Path to the CA public key file (PEM)
        #[arg(short = 'p', long)]
        ca_pubkey: PathBuf,
        /// Challenge that was sent (hex, 32 bytes)
        #[arg(short = 'c', long)]
        challenge_hex: String,
        /// Full attestation response from the card (hex)
        #[arg(short = 'r', long)]
        response_hex: String,
    },
    /// Personalize a card: generate card key, sign it, load certificate (requires NFC)
    #[cfg(feature = "nfc")]
    Personalize {
        /// Path to the CA private key file (PEM)
        #[arg(short = 'k', long)]
        ca_key: PathBuf,
    },
    /// Test a personalized card's attestation (requires NFC)
    #[cfg(feature = "nfc")]
    Test {
        /// Path to the CA public key file (PEM)
        #[arg(short = 'p', long)]
        ca_pubkey: PathBuf,
        /// Challenge hex string (32 bytes). Random if omitted.
        #[arg(short, long)]
        challenge: Option<String>,
    },
    /// Show card attestation status (requires NFC)
    #[cfg(feature = "nfc")]
    Status {
        /// Path to the CA public key file (PEM) for display
        #[arg(short = 'p', long)]
        ca_pubkey: Option<PathBuf>,
    },
}

/// APDU constants matching BrioletteApplet.java
#[cfg(feature = "nfc")]
mod apdu {
    pub const CLA: u8 = 0x80;
    pub const INS_MFR_GENERATE_KEY: u8 = 0x60;
    pub const INS_MFR_SET_CERT: u8 = 0x61;
    pub const INS_MFR_ATTEST: u8 = 0x62;
    pub const INS_GET_STATUS: u8 = 0x40;
}

/// Briolette applet AID
#[cfg(feature = "nfc")]
const BRIOLETTE_AID: [u8; 7] = [0xA0, 0x00, 0x00, 0x00, 0x62, 0x03, 0x01];

/// Hex-encode bytes.
fn hex_encode(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{:02x}", b)).collect()
}

/// Hex-decode a string.
fn hex_decode(s: &str) -> Result<Vec<u8>, String> {
    if s.len() % 2 != 0 {
        return Err("Hex string has odd length".to_string());
    }
    (0..s.len())
        .step_by(2)
        .map(|i| {
            u8::from_str_radix(&s[i..i + 2], 16)
                .map_err(|_| format!("Invalid hex at position {}", i))
        })
        .collect()
}

/// Compute a short fingerprint for a CA public key.
fn ca_fingerprint(key: &VerifyingKey) -> String {
    let bytes = key.to_encoded_point(false);
    let hash = Sha256::digest(bytes.as_bytes());
    hex_encode(&hash[..8])
}

/// Load a CA signing key from a PEM file.
fn load_ca_signing_key(path: &PathBuf) -> Result<SigningKey, String> {
    use elliptic_curve::pkcs8::DecodePrivateKey;
    let pem = std::fs::read_to_string(path)
        .map_err(|e| format!("Failed to read {}: {}", path.display(), e))?;
    SigningKey::from_pkcs8_pem(&pem)
        .map_err(|e| format!("Failed to parse CA private key: {:?}", e))
}

/// Load a CA verifying key from a PEM file.
fn load_ca_verifying_key(path: &PathBuf) -> Result<VerifyingKey, String> {
    use elliptic_curve::pkcs8::DecodePublicKey;
    let pem = std::fs::read_to_string(path)
        .map_err(|e| format!("Failed to read {}: {}", path.display(), e))?;
    VerifyingKey::from_public_key_pem(&pem)
        .map_err(|e| format!("Failed to parse CA public key: {:?}", e))
}

/// Parse an attestation response: [1B sig_len][DER sig][1B cert_len][cert][65B pubkey]
fn parse_attestation_response(resp: &[u8]) -> Result<(&[u8], &[u8], &[u8]), String> {
    if resp.is_empty() {
        return Err("Empty attestation response".to_string());
    }
    let sig_len = resp[0] as usize;
    if resp.len() < 1 + sig_len + 1 {
        return Err("Attestation response too short for signature".to_string());
    }
    let attest_sig = &resp[1..1 + sig_len];
    let cert_len = resp[1 + sig_len] as usize;
    if resp.len() < 1 + sig_len + 1 + cert_len + 65 {
        return Err(format!(
            "Attestation response too short: need {} bytes, got {}",
            1 + sig_len + 1 + cert_len + 65,
            resp.len()
        ));
    }
    let cert = &resp[1 + sig_len + 1..1 + sig_len + 1 + cert_len];
    let pubkey = &resp[1 + sig_len + 1 + cert_len..];
    if pubkey.len() != 65 {
        return Err(format!("Expected 65-byte public key, got {}", pubkey.len()));
    }
    Ok((attest_sig, cert, pubkey))
}

/// Verify an attestation response against a CA public key and challenge.
fn verify_attestation(
    ca_vk: &VerifyingKey,
    challenge: &[u8],
    attest_sig_bytes: &[u8],
    cert_bytes: &[u8],
    pubkey_bytes: &[u8],
) -> Result<(), String> {
    // Verify attestation signature (card signed the challenge)
    let attest_sig = DerSignature::try_from(attest_sig_bytes)
        .map_err(|e| format!("Failed to parse attestation signature: {:?}", e))?;
    let card_vk = VerifyingKey::from_sec1_bytes(pubkey_bytes)
        .map_err(|e| format!("Failed to parse card public key: {:?}", e))?;
    card_vk
        .verify(challenge, &attest_sig)
        .map_err(|e| format!("Attestation signature verification FAILED: {:?}", e))?;
    println!("  Attestation signature: VALID");

    // Verify manufacturer certificate (CA signed the card's public key)
    let cert_sig = DerSignature::try_from(cert_bytes)
        .map_err(|e| format!("Failed to parse certificate: {:?}", e))?;
    ca_vk
        .verify(pubkey_bytes, &cert_sig)
        .map_err(|e| format!("Certificate verification FAILED: {:?}", e))?;
    println!("  Manufacturer certificate: VALID");

    Ok(())
}

// ─── Commands that always work (no NFC required) ────────────────────────────

/// Generate a P-256 CA keypair and save to files.
fn cmd_generate_ca(output_dir: &PathBuf) -> Result<(), String> {
    use elliptic_curve::pkcs8::EncodePrivateKey;
    use elliptic_curve::pkcs8::EncodePublicKey;

    let signing_key = SigningKey::random(&mut rand::thread_rng());
    let verifying_key = signing_key.verifying_key();

    // Save private key as PEM
    let sk_path = output_dir.join("mfr_ca_key.pem");
    let sk_pem = signing_key
        .to_pkcs8_pem(elliptic_curve::pkcs8::LineEnding::LF)
        .map_err(|e| format!("Failed to encode private key: {:?}", e))?;
    std::fs::write(&sk_path, sk_pem.as_bytes())
        .map_err(|e| format!("Failed to write {}: {}", sk_path.display(), e))?;

    // Set file permissions to 0600 on unix
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&sk_path, std::fs::Permissions::from_mode(0o600))
            .map_err(|e| format!("Failed to set permissions: {}", e))?;
    }

    // Save public key as PEM
    let pk_path = output_dir.join("mfr_ca_pubkey.pem");
    let pk_pem = verifying_key
        .to_public_key_pem(elliptic_curve::pkcs8::LineEnding::LF)
        .map_err(|e| format!("Failed to encode public key: {:?}", e))?;
    std::fs::write(&pk_path, &pk_pem)
        .map_err(|e| format!("Failed to write {}: {}", pk_path.display(), e))?;

    println!("CA keypair generated:");
    println!("  Private key: {}", sk_path.display());
    println!("  Public key:  {}", pk_path.display());
    println!("  Fingerprint: {}", ca_fingerprint(verifying_key));

    Ok(())
}

/// Sign a card public key offline (for manual personalization workflows).
fn cmd_sign_key(ca_key_path: &PathBuf, card_pubkey_hex: &str) -> Result<(), String> {
    let ca_sk = load_ca_signing_key(ca_key_path)?;
    let ca_vk = ca_sk.verifying_key();
    println!("CA fingerprint: {}", ca_fingerprint(ca_vk));

    let card_pubkey = hex_decode(card_pubkey_hex)?;
    if card_pubkey.len() != 65 || card_pubkey[0] != 0x04 {
        return Err("Card public key must be 65-byte uncompressed SEC1 point (04 || x || y)".to_string());
    }

    // Verify it's a valid P-256 point
    VerifyingKey::from_sec1_bytes(&card_pubkey)
        .map_err(|e| format!("Invalid P-256 point: {:?}", e))?;

    // Sign the card's public key
    let signature: DerSignature = ca_sk.sign(&card_pubkey);
    let sig_bytes = signature.to_bytes();

    // Verify our own signature
    ca_vk
        .verify(&card_pubkey, &signature)
        .map_err(|e| format!("Self-verification failed (bug): {:?}", e))?;

    println!("Certificate (DER ECDSA signature over card pubkey):");
    println!("  Hex: {}", hex_encode(&sig_bytes));
    println!("  Length: {} bytes", sig_bytes.len());
    println!("\nTo load onto card via APDU:");
    println!("  80 61 00 00 {:02X} {}", sig_bytes.len(), hex_encode(&sig_bytes));

    Ok(())
}

/// Verify an attestation response offline.
fn cmd_verify_attestation(
    ca_pubkey_path: &PathBuf,
    challenge_hex: &str,
    response_hex: &str,
) -> Result<(), String> {
    let ca_vk = load_ca_verifying_key(ca_pubkey_path)?;
    println!("CA fingerprint: {}", ca_fingerprint(&ca_vk));

    let challenge = hex_decode(challenge_hex)?;
    if challenge.len() != 32 {
        return Err(format!("Challenge must be 32 bytes, got {}", challenge.len()));
    }

    let response = hex_decode(response_hex)?;
    let (attest_sig, cert, pubkey) = parse_attestation_response(&response)?;

    println!("Card public key: {}", hex_encode(pubkey));
    verify_attestation(&ca_vk, &challenge, attest_sig, cert, pubkey)
}

// ─── NFC commands (require pcsc feature) ────────────────────────────────────

#[cfg(feature = "nfc")]
mod nfc_ops {
    use super::*;

    /// Send an APDU to the card and return the response data.
    pub fn send_apdu(card: &pcsc::Card, ins: u8, p1: u8, p2: u8, data: &[u8]) -> Result<Vec<u8>, String> {
        let mut apdu_buf = Vec::with_capacity(5 + data.len() + 1);
        apdu_buf.push(apdu::CLA);
        apdu_buf.push(ins);
        apdu_buf.push(p1);
        apdu_buf.push(p2);
        apdu_buf.push(data.len() as u8);
        apdu_buf.extend_from_slice(data);
        apdu_buf.push(0x00); // Le

        let mut recv_buf = vec![0u8; 258];
        let response = card
            .transmit(&apdu_buf, &mut recv_buf)
            .map_err(|e| format!("PC/SC transmit error: {:?}", e))?;

        if response.len() < 2 {
            return Err("Response too short".to_string());
        }

        let sw1 = response[response.len() - 2];
        let sw2 = response[response.len() - 1];
        let sw = ((sw1 as u16) << 8) | (sw2 as u16);

        if sw != 0x9000 {
            return Err(format!("Card returned SW={:04X}", sw));
        }

        Ok(response[..response.len() - 2].to_vec())
    }

    /// Select the Briolette applet on the card.
    pub fn select_applet(card: &pcsc::Card) -> Result<(), String> {
        let mut apdu_buf = Vec::with_capacity(5 + BRIOLETTE_AID.len() + 1);
        apdu_buf.push(0x00); // CLA: ISO
        apdu_buf.push(0xA4); // INS: SELECT
        apdu_buf.push(0x04); // P1: Select by DF name
        apdu_buf.push(0x00); // P2
        apdu_buf.push(BRIOLETTE_AID.len() as u8);
        apdu_buf.extend_from_slice(&BRIOLETTE_AID);
        apdu_buf.push(0x00); // Le

        let mut recv_buf = vec![0u8; 258];
        let response = card
            .transmit(&apdu_buf, &mut recv_buf)
            .map_err(|e| format!("PC/SC transmit error: {:?}", e))?;

        if response.len() < 2 {
            return Err("SELECT response too short".to_string());
        }

        let sw = ((response[response.len() - 2] as u16) << 8) | (response[response.len() - 1] as u16);
        if sw != 0x9000 {
            return Err(format!("SELECT failed with SW={:04X}", sw));
        }

        log::info!("Briolette applet selected successfully");
        Ok(())
    }

    /// Connect to the first available NFC reader.
    pub fn connect_card() -> Result<pcsc::Card, String> {
        let ctx = pcsc::Context::establish(pcsc::Scope::System)
            .map_err(|e| format!("Failed to establish PC/SC context: {:?}", e))?;

        let mut readers_buf = vec![0u8; 2048];
        let readers: Vec<&std::ffi::CStr> = ctx
            .list_readers(&mut readers_buf)
            .map_err(|e| format!("Failed to list readers: {:?}", e))?
            .collect();

        if readers.is_empty() {
            return Err("No NFC readers found. Connect an NFC reader and place a card.".to_string());
        }

        log::info!("Found {} reader(s)", readers.len());
        for (i, r) in readers.iter().enumerate() {
            log::info!("  [{}] {:?}", i, r);
        }

        let card = ctx
            .connect(readers[0], pcsc::ShareMode::Shared, pcsc::Protocols::ANY)
            .map_err(|e| format!("Failed to connect to card: {:?}", e))?;

        println!("Connected to reader: {:?}", readers[0]);
        Ok(card)
    }

    /// Decode the GET_STATUS response flags.
    fn decode_status(status_byte: u8) -> Vec<&'static str> {
        let mut flags = Vec::new();
        if status_byte & 0x01 != 0 { flags.push("key_generated"); }
        if status_byte & 0x02 != 0 { flags.push("personalized"); }
        if status_byte & 0x04 != 0 { flags.push("joined"); }
        if status_byte & 0x08 != 0 { flags.push("credential_loaded"); }
        if status_byte & 0x10 != 0 { flags.push("swap_key_set"); }
        if status_byte & 0x20 != 0 { flags.push("mfr_key_generated"); }
        if status_byte & 0x40 != 0 { flags.push("mfr_cert_loaded"); }
        flags
    }

    /// Personalize a card: generate key, sign it, load cert.
    pub fn cmd_personalize(ca_key_path: &PathBuf) -> Result<(), String> {
        let ca_sk = load_ca_signing_key(ca_key_path)?;
        let ca_vk = ca_sk.verifying_key();
        println!("CA fingerprint: {}", ca_fingerprint(ca_vk));

        let card = connect_card()?;
        select_applet(&card)?;

        // Check current status
        let status = send_apdu(&card, apdu::INS_GET_STATUS, 0x00, 0x00, &[])?;
        if !status.is_empty() {
            let flags = decode_status(status[0]);
            log::info!("Card status: {:?}", flags);

            if status[0] & 0x40 != 0 {
                return Err("Card already has manufacturer cert. Cannot re-personalize.".to_string());
            }
        }

        // Step 1: Generate manufacturer key on card
        let card_pubkey = if status.is_empty() || status[0] & 0x20 == 0 {
            println!("Generating P-256 keypair on card...");
            let resp = send_apdu(&card, apdu::INS_MFR_GENERATE_KEY, 0x00, 0x00, &[])?;
            if resp.len() != 65 {
                return Err(format!("Expected 65-byte public key, got {} bytes", resp.len()));
            }
            println!("Card public key: {}", hex_encode(&resp));
            resp
        } else {
            return Err(
                "Card has manufacturer key but no cert. Public key was returned during \
                 generation. Re-flash the card to start fresh."
                    .to_string(),
            );
        };

        // Step 2: Sign the card's public key with the CA key
        println!("Signing card public key with CA key...");
        let signature: DerSignature = ca_sk.sign(&card_pubkey);
        let sig_bytes = signature.to_bytes();

        // Self-verify before loading
        ca_vk
            .verify(&card_pubkey, &signature)
            .map_err(|e| format!("Self-verification failed: {:?}", e))?;

        // Step 3: Load the certificate onto the card
        println!("Loading certificate onto card...");
        send_apdu(&card, apdu::INS_MFR_SET_CERT, 0x00, 0x00, &sig_bytes)?;
        println!("Certificate loaded.");

        // Step 4: Verify by running a test attestation
        println!("\nVerifying attestation...");
        let challenge: [u8; 32] = rand::random();
        let attest_resp = send_apdu(&card, apdu::INS_MFR_ATTEST, 0x00, 0x00, &challenge)?;

        let (attest_sig, cert, pubkey) = parse_attestation_response(&attest_resp)?;
        verify_attestation(ca_vk, &challenge, attest_sig, cert, pubkey)?;

        println!("\nCard personalization complete!");
        println!("  Card public key: {}", hex_encode(pubkey));
        println!("  CA fingerprint:  {}", ca_fingerprint(ca_vk));

        Ok(())
    }

    /// Test a personalized card's attestation.
    pub fn cmd_test(ca_pubkey_path: &PathBuf, challenge_hex: Option<&str>) -> Result<(), String> {
        let ca_vk = load_ca_verifying_key(ca_pubkey_path)?;
        println!("CA fingerprint: {}", ca_fingerprint(&ca_vk));

        let challenge: Vec<u8> = match challenge_hex {
            Some(hex) => {
                let bytes = hex_decode(hex)?;
                if bytes.len() != 32 {
                    return Err(format!("Challenge must be 32 bytes, got {}", bytes.len()));
                }
                bytes
            }
            None => {
                let c: [u8; 32] = rand::random();
                c.to_vec()
            }
        };
        println!("Challenge: {}", hex_encode(&challenge));

        let card = connect_card()?;
        select_applet(&card)?;

        let attest_resp = send_apdu(&card, apdu::INS_MFR_ATTEST, 0x00, 0x00, &challenge)?;
        let (attest_sig, cert, pubkey) = parse_attestation_response(&attest_resp)?;

        println!("\nAttestation response:");
        println!("  Card public key: {}", hex_encode(pubkey));
        verify_attestation(&ca_vk, &challenge, attest_sig, cert, pubkey)
    }

    /// Show card attestation status.
    pub fn cmd_status(ca_pubkey_path: Option<&PathBuf>) -> Result<(), String> {
        if let Some(path) = ca_pubkey_path {
            let ca_vk = load_ca_verifying_key(path)?;
            println!("CA fingerprint: {}", ca_fingerprint(&ca_vk));
            println!();
        }

        let card = connect_card()?;
        select_applet(&card)?;

        let status = send_apdu(&card, apdu::INS_GET_STATUS, 0x00, 0x00, &[])?;
        if status.is_empty() {
            println!("Card returned empty status");
            return Ok(());
        }

        let flags = decode_status(status[0]);
        println!("Card status: 0x{:02X}", status[0]);
        println!("Flags: {:?}", flags);
        println!();
        println!(
            "Manufacturer key: {}",
            if status[0] & 0x20 != 0 { "GENERATED" } else { "NOT GENERATED" }
        );
        println!(
            "Manufacturer cert: {}",
            if status[0] & 0x40 != 0 { "LOADED" } else { "NOT LOADED" }
        );
        println!(
            "Personalization lock: {}",
            if status[0] & 0x02 != 0 { "ACTIVE" } else { "INACTIVE" }
        );

        Ok(())
    }
}

fn main() {
    let cli = Cli::parse();

    stderrlog::new()
        .module(module_path!())
        .verbosity(cli.verbose as usize + 1)
        .init()
        .unwrap();

    let result = match &cli.command {
        Commands::GenerateCa { output_dir } => cmd_generate_ca(output_dir),
        Commands::SignKey {
            ca_key,
            card_pubkey_hex,
        } => cmd_sign_key(ca_key, card_pubkey_hex),
        Commands::VerifyAttestation {
            ca_pubkey,
            challenge_hex,
            response_hex,
        } => cmd_verify_attestation(ca_pubkey, challenge_hex, response_hex),
        #[cfg(feature = "nfc")]
        Commands::Personalize { ca_key } => nfc_ops::cmd_personalize(ca_key),
        #[cfg(feature = "nfc")]
        Commands::Test {
            ca_pubkey,
            challenge,
        } => nfc_ops::cmd_test(ca_pubkey, challenge.as_deref()),
        #[cfg(feature = "nfc")]
        Commands::Status { ca_pubkey } => nfc_ops::cmd_status(ca_pubkey.as_ref()),
    };

    if let Err(e) = result {
        eprintln!("Error: {}", e);
        std::process::exit(1);
    }
}
