package com.briolette.wallet.data

/**
 * Wallet balance summary.
 */
data class Balance(
    val whole: Int = 0,
    val fractional: Int = 0,
    val currency: String = "TEST",
    val tokenCount: Int = 0,
) {
    /** Format as "42.50" style string. */
    val displayAmount: String
        get() {
            val frac = fractional / 10_000  // micros → 2 decimal places
            return if (frac > 0) "$whole.${frac.toString().padStart(2, '0')}"
            else whole.toString()
        }
}

/**
 * Immutable wallet state snapshot. Backed by serialized JSON from the Rust FFI.
 */
data class WalletState(
    val json: String = "",
    val balance: Balance = Balance(),
    val ticketCount: Int = 0,
    val walletName: String = "",
) {
    val isInitialized: Boolean get() = json.isNotBlank()
    val canPay: Boolean get() = balance.tokenCount > 0
    val canReceive: Boolean get() = ticketCount > 0
}

/**
 * Result of a token transfer operation.
 */
data class TransferResult(
    val state: WalletState,
    /** Base64-encoded tokens to deliver to the recipient (via QR, NFC, etc.) */
    val tokensBase64: List<String>,
)

/**
 * Result of a validation operation.
 */
data class ValidationResult(
    val state: WalletState,
    val allValid: Boolean,
    val validCount: Int,
    val invalidCount: Int,
)

/**
 * Server configuration for connecting to the Briolette network.
 */
data class NetworkConfig(
    val registrarUri: String = "http://127.0.0.1:50051",
    val clerkUri: String = "http://127.0.0.1:50052",
    val mintUri: String = "http://127.0.0.1:50053",
    val validateUri: String = "http://127.0.0.1:50055",
)

/**
 * Hardware attestation data for registration.
 *
 * Android: algorithm=1, signatureB64=length-prefixed DER cert chain, publicKeyB64=attested key
 * iOS:     algorithm=2, signatureB64=CBOR attestation object, publicKeyB64=key identifier
 */
data class HwAttestationData(
    val algorithm: Int = 0,
    val signatureB64: String = "",
    val publicKeyB64: String = "",
)

/**
 * Platform-specific hardware attestation provider.
 * Implemented on Android using KeyStore and on iOS using DCAppAttestService.
 */
interface HwAttestationProvider {
    /** Whether this platform supports hardware attestation. */
    val isSupported: Boolean

    /**
     * Generate attestation data using the attestation challenge preimage.
     *
     * The `challengePreimageB64` is the base64-encoded preimage
     * `hw_id || nac_pk || ttc_pk`. The provider must SHA-256 hash this
     * to get the actual attestation challenge, which cryptographically
     * binds the hardware attestation to the ECDAA credential public keys.
     *
     * Returns null on failure.
     */
    suspend fun generate(challengePreimageB64: String): HwAttestationData?
}

/**
 * QR code payload types used in the app.
 */
sealed class QrPayload {
    /** A receiving ticket (SignedTicket) for incoming payments. */
    data class ReceivingTicket(val base64: String) : QrPayload()

    /** Token data being sent as payment. */
    data class TokenTransfer(val tokensBase64: List<String>) : QrPayload()
}
