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

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("cargo:rerun-if-changed=build.rs");

    let serde_attr =
        "#[derive(serde::Serialize, serde::Deserialize)]";

    // All proto files that need rerun tracking.
    let protos = [
        "proto/token.proto",
        "proto/common.proto",
        "proto/amount_type.proto",
        "proto/tokenmap.proto",
        "proto/mint.proto",
        "proto/clerk.proto",
        "proto/registrar.proto",
        "proto/validate.proto",
        "proto/receiver.proto",
        "proto/service_auth.proto",
        "proto/bridge.proto",
        "proto/swapper.proto",
    ];
    for proto in &protos {
        println!("cargo:rerun-if-changed={}", proto);
    }

    // Apply serde derives to generated MESSAGE types for JSON serialization
    // in the HTTP/A2A/x402 transport layers.
    //
    // IMPORTANT: Do NOT add serde to prost ENUM types (Version, AmountType,
    // ErrorCode, SelectGroup, etc.). Prost represents enum fields as i32 in
    // message structs, so message-level serde handles them as integers. Adding
    // serde directly to enum types causes .into() ambiguity with serde_json.
    //
    // Note: bytes fields (Vec<u8>) serialize as arrays by default; the HTTP
    // layer handles base64 encoding via wrapper types in http_common.
    tonic_build::configure()
        // token package — message types only
        .type_attribute("briolette.token.Ticket", serde_attr)
        .type_attribute("briolette.token.TicketData", serde_attr)
        .type_attribute("briolette.token.SignedTicket", serde_attr)
        .type_attribute("briolette.token.Amount", serde_attr)
        .type_attribute("briolette.token.Descriptor", serde_attr)
        .type_attribute("briolette.token.Tag", serde_attr)
        .type_attribute("briolette.token.Tag.value", serde_attr)
        .type_attribute("briolette.token.Transfer", serde_attr)
        .type_attribute("briolette.token.History", serde_attr)
        .type_attribute("briolette.token.Token", serde_attr)
        // receiver package
        .type_attribute("briolette.receiver.InitiateRequest", serde_attr)
        .type_attribute("briolette.receiver.InitiateReply", serde_attr)
        .type_attribute("briolette.receiver.TransactionItem", serde_attr)
        .type_attribute("briolette.receiver.GossipRequest", serde_attr)
        .type_attribute("briolette.receiver.GossipReply", serde_attr)
        .type_attribute("briolette.receiver.TransactRequest", serde_attr)
        .type_attribute("briolette.receiver.TransactionItemMethod", serde_attr)
        .type_attribute("briolette.receiver.TransactReply", serde_attr)
        .type_attribute("briolette.receiver.TransferRequest", serde_attr)
        .type_attribute("briolette.receiver.TransferReply", serde_attr)
        .type_attribute("briolette.receiver.AbortRequest", serde_attr)
        .type_attribute("briolette.receiver.AbortReply", serde_attr)
        // common package — messages only
        .type_attribute("briolette.Error", serde_attr)
        .type_attribute("briolette.ServiceMapEntry", serde_attr)
        .type_attribute("briolette.ServiceMap", serde_attr)
        // clerk types (used by receiver)
        .type_attribute("briolette.clerk.EpochData", serde_attr)
        .type_attribute("briolette.clerk.EpochUpdate", serde_attr)
        .type_attribute("briolette.clerk.EpochVerify", serde_attr)
        .type_attribute("briolette.clerk.ExtendedEpochData", serde_attr)
        .type_attribute("briolette.clerk.GroupPolicy", serde_attr)
        // bridge types — messages only
        .type_attribute("briolette.bridge.L1Deposit", serde_attr)
        .type_attribute("briolette.bridge.WithdrawRequest", serde_attr)
        .type_attribute("briolette.bridge.WithdrawReply", serde_attr)
        .type_attribute("briolette.bridge.WithdrawalStatusReply", serde_attr)
        // swapper types
        .type_attribute("briolette.swapper.GetDestinationReply", serde_attr)
        .type_attribute("briolette.swapper.SwapTokensRequest", serde_attr)
        .type_attribute("briolette.swapper.SwapTokensReply", serde_attr)
        .compile(&protos, &["proto"])?;

    Ok(())
}
