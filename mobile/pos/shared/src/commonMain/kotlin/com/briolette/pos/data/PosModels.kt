package com.briolette.pos.data

/**
 * PoS terminal state. The PoS holds a merchant's SignedTicket (stored once
 * during setup) and accumulates received tokens for later sweep.
 */
data class PosState(
    /** Whether a merchant ticket has been configured. */
    val merchantConfigured: Boolean = false,
    /** Merchant's SignedTicket (serialized protobuf, base64). */
    val merchantTicketB64: String = "",
    /** Cached epoch data (serialized protobuf, base64). */
    val epochDataB64: String = "",
    /** Current epoch number. */
    val epochNumber: Int = 0,
    /** Total tokens accumulated (validated + unvalidated). */
    val totalAccumulated: Int = 0,
    /** Count of validated token batches. */
    val validatedCount: Int = 0,
    /** Count of unvalidated token batches. */
    val unvalidatedCount: Int = 0,
    /** Whether the PoS has internet connectivity. */
    val isOnline: Boolean = false,
)

/**
 * A single received payment (transaction record).
 */
data class PaymentRecord(
    val id: String,
    val timestamp: Long,
    val amount: Int,
    val description: String,
    /** Validation status: PENDING, VALID, INVALID. */
    val validationStatus: ValidationStatus,
    /** Whether this payment has been swept to the merchant. */
    val swept: Boolean = false,
)

enum class ValidationStatus {
    PENDING,
    VALID,
    INVALID,
}

/**
 * Transaction flow phase (tracks multi-tap NFC interaction).
 */
sealed class TransactionPhase {
    /** Ready for a new payment. */
    data object Idle : TransactionPhase()

    /** Merchant entered an amount, waiting for customer tap. */
    data class WaitingForTap(val amount: Int, val description: String) : TransactionPhase()

    /** Tap 1 complete: proposal sent, unsigned tokens received.
     *  Validating tokens while waiting for tap 2. */
    data class ProposalSent(
        val amount: Int,
        val txId: ByteArray,
        val unsignedTokens: ByteArray,
        val cryptoValid: Boolean = false,
        val onlineValid: Boolean? = null,  // null = checking, true/false = result
    ) : TransactionPhase()

    /** Tap 2 complete: signed tokens received. Payment done. */
    data class Complete(
        val amount: Int,
        val validated: Boolean,
    ) : TransactionPhase()

    /** Transaction failed or rejected. */
    data class Failed(val reason: String) : TransactionPhase()
}

/**
 * NFC terminal abstraction for credstick communication.
 *
 * Platform-specific implementations use Android IsoDep or iOS CoreNFC.
 * The PoS acts as an NFC reader (initiator), unlike the wallet app
 * where the phone acts as a tag.
 */
interface NfcTerminal {
    /** Whether NFC reader mode is available. */
    val isAvailable: Boolean

    /** Enable reader mode and wait for a tag. Returns raw tag handle. */
    suspend fun waitForTag(): NfcTag?

    /** Disable reader mode. */
    suspend fun stopReading()
}

/**
 * Represents a connected NFC tag (credstick).
 */
interface NfcTag {
    /** Send an APDU and receive the response. */
    suspend fun transceive(apdu: ByteArray): ByteArray

    /** Whether the tag is still connected. */
    val isConnected: Boolean

    /** Close the connection. */
    suspend fun close()
}
