package com.briolette.pos.data

import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow

/**
 * Central state management for the PoS terminal.
 *
 * Manages:
 * - Merchant setup (one-time credstick tap to store ticket)
 * - Transaction lifecycle (propose → validate → confirm)
 * - Token accumulation and sweep
 * - Online validation when connected
 */
class PosRepository(
    private val persistence: PosPersistence,
    private val onlineValidator: OnlineValidator?,
) {
    private val _state = MutableStateFlow(PosState())
    val state: StateFlow<PosState> = _state.asStateFlow()

    private val _phase = MutableStateFlow<TransactionPhase>(TransactionPhase.Idle)
    val phase: StateFlow<TransactionPhase> = _phase.asStateFlow()

    private val _recentPayments = MutableStateFlow<List<PaymentRecord>>(emptyList())
    val recentPayments: StateFlow<List<PaymentRecord>> = _recentPayments.asStateFlow()

    init {
        // Load persisted state.
        val saved = persistence.loadState()
        if (saved != null) {
            _state.value = saved
        }
        _recentPayments.value = persistence.loadRecentPayments()
    }

    // --- Setup ---

    /**
     * Store a merchant's SignedTicket (from credstick tap during setup).
     */
    fun setupMerchant(ticketB64: String, epochDataB64: String, epochNumber: Int) {
        _state.value = _state.value.copy(
            merchantConfigured = true,
            merchantTicketB64 = ticketB64,
            epochDataB64 = epochDataB64,
            epochNumber = epochNumber,
        )
        persistence.saveState(_state.value)
    }

    // --- Transaction Flow ---

    /**
     * Start a new payment (merchant enters amount).
     */
    fun startPayment(amount: Int, description: String = "") {
        _phase.value = TransactionPhase.WaitingForTap(amount, description)
    }

    /**
     * Execute Tap 1: Send INITIATE + TRANSACT to the credstick.
     *
     * In 2-tap fast mode:
     * 1. SELECT Briolette AID
     * 2. INITIATE(ticket, amount, items, epoch)
     * 3. Parse response: tx_id + unsigned tokens
     * 4. Begin online validation of unsigned tokens
     *
     * Returns true if tap 1 succeeded.
     */
    suspend fun executeTap1(tag: NfcTag): Boolean {
        val currentPhase = _phase.value
        if (currentPhase !is TransactionPhase.WaitingForTap) return false
        val stateSnapshot = _state.value

        try {
            // SELECT AID.
            val selectResp = tag.transceive(ApduProtocol.selectApdu())
            if (!ApduProtocol.Sw.isSuccess(ApduProtocol.extractSw(selectResp))) {
                _phase.value = TransactionPhase.Failed("Credstick not recognized")
                return false
            }

            // INITIATE: send proposal.
            val ticketData = decodeBase64(stateSnapshot.merchantTicketB64)
            val epochData = decodeBase64(stateSnapshot.epochDataB64)
            val initiateApdu = ApduProtocol.initiateApdu(
                amount = currentPhase.amount,
                description = currentPhase.description,
                ticketData = ticketData,
                epochData = epochData,
            )
            val initiateResp = tag.transceive(initiateApdu)
            val initSw = ApduProtocol.extractSw(initiateResp)

            if (!ApduProtocol.Sw.isSuccess(initSw)) {
                _phase.value = TransactionPhase.Failed("Credstick rejected proposal (SW: ${initSw.toString(16)})")
                return false
            }

            val initData = ApduProtocol.extractData(initiateResp)

            // Parse response: first 16 bytes = tx_id, rest = unsigned tokens (2-tap mode).
            if (initData.size < 16) {
                _phase.value = TransactionPhase.Failed("Invalid INITIATE response")
                return false
            }
            val txId = initData.copyOfRange(0, 16)
            val unsignedTokens = initData.copyOfRange(16, initData.size)

            // Transition to ProposalSent.
            _phase.value = TransactionPhase.ProposalSent(
                amount = currentPhase.amount,
                txId = txId,
                unsignedTokens = unsignedTokens,
            )

            // Start online validation in background (if connected).
            if (unsignedTokens.isNotEmpty()) {
                validateTokensAsync(unsignedTokens)
            }

            tag.close()
            return true
        } catch (e: Exception) {
            _phase.value = TransactionPhase.Failed("NFC error: ${e.message}")
            return false
        }
    }

    /**
     * Execute Tap 2: Send TRANSFER to the credstick.
     *
     * 1. SELECT Briolette AID
     * 2. TRANSFER(tx_id, accept=true)
     * 3. Receive signed tokens (BLS signatures)
     * 4. Store in local token accumulation
     *
     * Returns true if payment completed.
     */
    suspend fun executeTap2(tag: NfcTag): Boolean {
        val currentPhase = _phase.value
        if (currentPhase !is TransactionPhase.ProposalSent) return false

        try {
            // SELECT AID.
            val selectResp = tag.transceive(ApduProtocol.selectApdu())
            if (!ApduProtocol.Sw.isSuccess(ApduProtocol.extractSw(selectResp))) {
                _phase.value = TransactionPhase.Failed("Credstick not recognized on tap 2")
                return false
            }

            // Determine accept/reject based on validation results.
            val accept = currentPhase.onlineValid != false  // Accept unless proven invalid.

            // TRANSFER: request signatures.
            val transferApdu = ApduProtocol.transferApdu(currentPhase.txId, accept)
            val transferResp = tag.transceive(transferApdu)
            val transferSw = ApduProtocol.extractSw(transferResp)

            // Handle PIN_REQUIRED response.
            if (ApduProtocol.Sw.isPinRequired(transferSw)) {
                val retries = ApduProtocol.Sw.pinRetries(transferSw)
                _phase.value = TransactionPhase.Failed("PIN required on credstick ($retries retries left)")
                return false
            }

            if (ApduProtocol.Sw.isSuccess(transferSw)) {
                val signedTokens = ApduProtocol.extractData(transferResp)

                // Combine unsigned tokens + signatures into complete tokens.
                val completeTokens = combineTokensWithSignatures(
                    currentPhase.unsignedTokens,
                    signedTokens,
                )

                // Store in local accumulation.
                val record = PaymentRecord(
                    id = currentPhase.txId.toHexString(),
                    timestamp = currentTimeMillis(),
                    amount = currentPhase.amount,
                    description = "",
                    validationStatus = if (currentPhase.onlineValid == true)
                        ValidationStatus.VALID else ValidationStatus.PENDING,
                )
                persistence.savePayment(record, completeTokens)

                // Update totals.
                _state.value = _state.value.copy(
                    totalAccumulated = _state.value.totalAccumulated + currentPhase.amount,
                    validatedCount = if (currentPhase.onlineValid == true)
                        _state.value.validatedCount + 1 else _state.value.validatedCount,
                    unvalidatedCount = if (currentPhase.onlineValid != true)
                        _state.value.unvalidatedCount + 1 else _state.value.unvalidatedCount,
                )
                persistence.saveState(_state.value)

                // Refresh recent payments list.
                _recentPayments.value = persistence.loadRecentPayments()

                _phase.value = TransactionPhase.Complete(
                    amount = currentPhase.amount,
                    validated = currentPhase.onlineValid == true,
                )

                tag.close()
                return true
            }

            if (transferSw == ApduProtocol.Sw.LOCKED) {
                _phase.value = TransactionPhase.Failed("Credstick is locked (PIN exhausted)")
            } else {
                _phase.value = TransactionPhase.Failed("TRANSFER failed (SW: ${transferSw.toString(16)})")
            }

            tag.close()
            return false
        } catch (e: Exception) {
            _phase.value = TransactionPhase.Failed("NFC error on tap 2: ${e.message}")
            return false
        }
    }

    /**
     * Sweep accumulated tokens to merchant credstick.
     */
    suspend fun sweepToCredstick(tag: NfcTag): Boolean {
        try {
            val selectResp = tag.transceive(ApduProtocol.selectApdu())
            if (!ApduProtocol.Sw.isSuccess(ApduProtocol.extractSw(selectResp))) {
                return false
            }

            val tokens = persistence.loadUnsweptTokens()
            val receiveApdu = ApduProtocol.receiveApdu(tokens)
            val resp = tag.transceive(receiveApdu)
            val sw = ApduProtocol.extractSw(resp)

            if (ApduProtocol.Sw.isSuccess(sw)) {
                persistence.markSwept()
                _state.value = _state.value.copy(
                    totalAccumulated = 0,
                    validatedCount = 0,
                    unvalidatedCount = 0,
                )
                persistence.saveState(_state.value)
                _recentPayments.value = persistence.loadRecentPayments()
                return true
            }

            return false
        } catch (e: Exception) {
            return false
        }
    }

    /**
     * Reset to idle state (for "New Payment" button).
     */
    fun resetTransaction() {
        _phase.value = TransactionPhase.Idle
    }

    // --- Online Validation ---

    private suspend fun validateTokensAsync(unsignedTokens: ByteArray) {
        val validator = onlineValidator ?: return
        try {
            val result = validator.checkTokens(unsignedTokens)
            val current = _phase.value
            if (current is TransactionPhase.ProposalSent) {
                _phase.value = current.copy(
                    cryptoValid = result.cryptoValid,
                    onlineValid = result.onlineValid,
                )
            }
        } catch (_: Exception) {
            // Validation failed — continue without online check.
        }
    }

    /**
     * Batch-validate unvalidated tokens when connectivity is restored.
     */
    suspend fun batchValidate() {
        val validator = onlineValidator ?: return
        val pending = persistence.loadPendingValidation()
        for (record in pending) {
            try {
                val tokens = persistence.loadTokenData(record.id)
                val result = validator.checkTokens(tokens)
                val newStatus = if (result.onlineValid == true)
                    ValidationStatus.VALID else ValidationStatus.INVALID
                persistence.updateValidationStatus(record.id, newStatus)
            } catch (_: Exception) {
                // Skip — retry next time.
            }
        }
        _recentPayments.value = persistence.loadRecentPayments()
    }

    // --- Helpers ---

    private fun combineTokensWithSignatures(
        unsigned: ByteArray,
        signatures: ByteArray,
    ): ByteArray {
        // Combine unsigned token data with BLS signatures to form complete tokens.
        // Format: [unsigned_data][signatures]
        // TODO: Proper protobuf Token assembly.
        return unsigned + signatures
    }

    private fun decodeBase64(input: String): ByteArray {
        // Platform-specific base64 decoding.
        // On Android: android.util.Base64
        // On iOS: Foundation Data(base64Encoded:)
        // For now, stub.
        return ByteArray(0) // TODO: platform-specific implementation
    }
}

// --- Platform-expected helpers ---

expect fun currentTimeMillis(): Long
expect fun ByteArray.toHexString(): String
