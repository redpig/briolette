import Foundation
import DeviceCheck
import CryptoKit

/// Generates iOS App Attest data for hardware-backed registration.
///
/// Uses Apple's `DCAppAttestService` to generate an attested key and produce
/// an attestation object. The registrar verifies the x5c certificate chain
/// in the CBOR attestation object against Apple's App Attest root CA and
/// checks that the nonce (SHA-256 of authData + clientDataHash) matches
/// the expected value derived from hw_id and the key identifier.
///
/// Requires iOS 14.0+ and a device with Secure Enclave.
@available(iOS 14.0, *)
class AppAttestHelper {

    private let service = DCAppAttestService.shared

    /// Check if App Attest is supported on this device.
    var isSupported: Bool {
        return service.isSupported
    }

    /// Generate App Attest attestation data for registration.
    ///
    /// - Parameter challenge: The attestation challenge (typically hw_id bytes).
    /// - Returns: Tuple of (algorithm, signatureB64, publicKeyB64) for HwAttestationData.
    /// - Throws: If key generation or attestation fails.
    func generateAttestation(challenge: Data) async throws -> (algorithm: Int32, signatureB64: String, publicKeyB64: String) {
        guard service.isSupported else {
            throw AppAttestError.notSupported
        }

        // 1. Generate a new attestation key.
        let keyId = try await service.generateKey()

        // 2. Create the client data hash from the challenge.
        //    The registrar expects: nonce = SHA-256(authData || SHA-256(hw_id || public_key))
        //    For attestation, clientDataHash = SHA-256(challenge) is standard.
        let clientDataHash = SHA256.hash(data: challenge)
        let clientDataHashData = Data(clientDataHash)

        // 3. Request attestation from the Secure Enclave.
        let attestationObject = try await service.attestKey(keyId, clientDataHash: clientDataHashData)

        // 4. Encode results as base64 for the FFI.
        let signatureB64 = attestationObject.base64EncodedString()
        let publicKeyB64 = Data(keyId.utf8).base64EncodedString()

        return (
            algorithm: 2,  // IOS_APP_ATTEST
            signatureB64: signatureB64,
            publicKeyB64: publicKeyB64
        )
    }
}

enum AppAttestError: Error, LocalizedError {
    case notSupported

    var errorDescription: String? {
        switch self {
        case .notSupported:
            return "App Attest is not supported on this device"
        }
    }
}
