package com.briolette.wallet.data

import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow

/**
 * Bridge interface to the Rust FFI wallet operations.
 *
 * Platform-specific implementations (Android/iOS) call into the UniFFI-generated
 * Kotlin/Swift bindings. This interface allows the common UI layer to remain
 * platform-agnostic.
 */
/**
 * Result of 2-phase key initialization.
 */
data class KeyInitResult(
    val walletJson: String,
    val challengePreimageB64: String,
    val nacCardPublicKeyB64: String = "",
    val ttcCardPublicKeyB64: String = "",
)

interface WalletBridge {
    suspend fun createWallet(name: String, config: NetworkConfig): WalletState
    suspend fun createWalletWithAttestation(
        name: String,
        config: NetworkConfig,
        attestation: HwAttestationData,
    ): WalletState {
        // Default: fall back to non-attested creation for platforms that don't support it yet.
        return createWallet(name, config)
    }

    /** Phase 1: init keys, get attestation challenge preimage. */
    suspend fun initWalletKeys(name: String, config: NetworkConfig): KeyInitResult {
        throw UnsupportedOperationException("2-phase registration not supported")
    }

    /** Phase 2: complete registration with cryptographically-bound attestation.
     *  Card public key fields are empty for MEDIUM mode, populated for HIGH mode.
     *  cardAttestationB64 is the base64-encoded MFR_ATTEST response from a
     *  personalized NFC card (empty if no card attestation). */
    suspend fun registerWalletWithAttestation(
        walletJson: String,
        attestation: HwAttestationData,
        nacCardPublicKeyB64: String = "",
        ttcCardPublicKeyB64: String = "",
        cardAttestationB64: String = "",
    ): WalletState {
        throw UnsupportedOperationException("2-phase registration not supported")
    }

    // ---- Split-key protocol (HIGH security) ----

    /** Split-key step 1: compute TTC base point for NFC card. */
    suspend fun splitKeyStart(name: String, config: NetworkConfig): SplitKeyStep1Result {
        throw UnsupportedOperationException("split-key not supported")
    }

    /** Split-key step 2a: after TTC card commit, get TTC challenge + NAC base. */
    suspend fun splitKeyAfterTtcCommit(
        stateJson: String, qCardTtcB64: String, uCardTtcB64: String,
    ): SplitKeyStep2aResult {
        throw UnsupportedOperationException("split-key not supported")
    }

    /** Split-key step 2b: after NAC card commit, get NAC challenge. */
    suspend fun splitKeyAfterNacCommit(
        stateJson: String, qCardNacB64: String, uCardNacB64: String,
    ): SplitKeyStep2bResult {
        throw UnsupportedOperationException("split-key not supported")
    }

    /** Split-key step 3: finalize keys with card response scalars. */
    suspend fun splitKeyComplete(
        stateJson: String, sCardTtcB64: String, sCardNacB64: String,
    ): KeyInitResult {
        throw UnsupportedOperationException("split-key not supported")
    }

    suspend fun loadWallet(json: String): WalletState
    suspend fun saveWallet(state: WalletState): String
    suspend fun synchronize(state: WalletState): WalletState
    suspend fun requestTickets(state: WalletState, count: Int): WalletState
    suspend fun withdraw(state: WalletState, amount: Int): WalletState
    suspend fun transfer(state: WalletState, recipientTicketB64: String, amount: Int): TransferResult
    suspend fun receiveTokens(state: WalletState, tokensB64: List<String>): WalletState
    suspend fun validate(state: WalletState): ValidationResult
    suspend fun getReceivingTicketB64(state: WalletState): String
}

/**
 * Central wallet repository managing state and coordinating operations.
 *
 * All UI screens observe [state] and call mutation methods. The repository
 * ensures operations are serialized (no concurrent mutations) and persists
 * state after each operation.
 */
class WalletRepository(
    private val bridge: WalletBridge,
    private val persistence: WalletPersistence,
    val attestationProvider: HwAttestationProvider? = null,
) {
    private val _state = MutableStateFlow(WalletState())
    val state: StateFlow<WalletState> = _state.asStateFlow()

    private val _isLoading = MutableStateFlow(false)
    val isLoading: StateFlow<Boolean> = _isLoading.asStateFlow()

    private val _error = MutableStateFlow<String?>(null)
    val error: StateFlow<String?> = _error.asStateFlow()

    fun clearError() {
        _error.value = null
    }

    /** Try to load a previously saved wallet. */
    suspend fun tryLoadSaved(): Boolean {
        val json = persistence.load() ?: return false
        return try {
            _state.value = bridge.loadWallet(json)
            true
        } catch (e: Exception) {
            false
        }
    }

    /** Create a new wallet and register with the network.
     *
     * MEDIUM mode: init keys → attest with ECDAA-bound challenge → register.
     * HIGH mode:   split-key with NFC card → attest with card-derived keys → register.
     * Falls back to unattested registration if attestation is unavailable.
     */
    suspend fun createWallet(
        name: String,
        config: NetworkConfig,
        securityMode: SecurityMode = SecurityMode.MEDIUM,
        nfcCardProvider: NfcCardProvider? = null,
    ) {
        withLoading {
            val provider = attestationProvider
            val newState = when {
                // HIGH security: split-key with NFC card + attestation
                securityMode == SecurityMode.HIGH && nfcCardProvider != null && provider != null -> {
                    createWalletHighSecurity(name, config, nfcCardProvider, provider)
                }
                // MEDIUM security: attestation only
                provider != null && provider.isSupported -> {
                    createWalletMediumSecurity(name, config, provider)
                }
                // No attestation: unattested fallback
                else -> {
                    bridge.createWallet(name, config)
                }
            }
            _state.value = newState
            persistence.save(newState.json)
        }
    }

    /** MEDIUM security: phone attestation binds to ECDAA keys. */
    private suspend fun createWalletMediumSecurity(
        name: String,
        config: NetworkConfig,
        provider: HwAttestationProvider,
    ): WalletState {
        // 1. Init keys → get challenge preimage (hw_id || nac_pk || ttc_pk)
        val keyInit = bridge.initWalletKeys(name, config)
        // 2. Provider decodes + SHA-256 hashes preimage, generates attestation
        val attestation = provider.generate(keyInit.challengePreimageB64)
        return if (attestation != null) {
            // 3. Complete registration with bound attestation (no card keys)
            bridge.registerWalletWithAttestation(keyInit.walletJson, attestation)
        } else {
            bridge.createWallet(name, config)
        }
    }

    /** HIGH security: NFC card split-key + phone attestation. */
    private suspend fun createWalletHighSecurity(
        name: String,
        config: NetworkConfig,
        nfcCardProvider: NfcCardProvider,
        attestationProvider: HwAttestationProvider,
    ): WalletState {
        // Step 1: compute TTC base point
        val step1 = bridge.splitKeyStart(name, config)

        // Step 1→card: send TTC base point, get card's TTC commit
        val (qCardTtc, uCardTtc) = nfcCardProvider.commitWithCard(step1.bTtcB64)

        // Step 2a: process TTC commit, get TTC challenge + NAC base
        val step2a = bridge.splitKeyAfterTtcCommit(step1.stateJson, qCardTtc, uCardTtc)

        // Step 2a→card: send NAC base point, get card's NAC commit
        val (qCardNac, uCardNac) = nfcCardProvider.commitWithCard(step2a.bNacB64)

        // Step 2b: process NAC commit, get NAC challenge
        val step2b = bridge.splitKeyAfterNacCommit(step2a.stateJson, qCardNac, uCardNac)

        // Step 2b→card: send both challenges, get card's response scalars
        val (sCardTtc, sCardNac) = nfcCardProvider.respondWithCard(step2a.cTtcB64, step2b.cNacB64)

        // Step 3: finalize keys with card responses → KeyInitResult with card public keys
        val keyInit = bridge.splitKeyComplete(step2b.stateJson, sCardTtc, sCardNac)

        // Attest with challenge preimage bound to card-derived combined public keys
        val attestation = attestationProvider.generate(keyInit.challengePreimageB64)
            ?: throw Exception("Hardware attestation required for HIGH security mode")

        // Request card manufacturer attestation (if card supports it)
        val cardAttestB64 = nfcCardProvider.getCardAttestation(keyInit.challengePreimageB64) ?: ""

        // Register with attestation + card public key shares + optional card attestation
        return bridge.registerWalletWithAttestation(
            keyInit.walletJson,
            attestation,
            nacCardPublicKeyB64 = keyInit.nacCardPublicKeyB64,
            ttcCardPublicKeyB64 = keyInit.ttcCardPublicKeyB64,
            cardAttestationB64 = cardAttestB64,
        )
    }

    /** Sync epoch data from the clerk. */
    suspend fun synchronize() {
        withLoading {
            val updated = bridge.synchronize(_state.value)
            _state.value = updated
            persistence.save(updated.json)
        }
    }

    /** Request more receiving tickets. */
    suspend fun requestTickets(count: Int = 5) {
        withLoading {
            val updated = bridge.requestTickets(_state.value, count)
            _state.value = updated
            persistence.save(updated.json)
        }
    }

    /** Top up: withdraw tokens from the mint. */
    suspend fun topUp(amount: Int) {
        withLoading {
            val updated = bridge.withdraw(_state.value, amount)
            _state.value = updated
            persistence.save(updated.json)
        }
    }

    /** Pay: transfer tokens to a recipient's ticket. */
    suspend fun pay(recipientTicketB64: String, amount: Int): List<String> {
        return withLoading {
            val result = bridge.transfer(_state.value, recipientTicketB64, amount)
            _state.value = result.state
            persistence.save(result.state.json)
            result.tokensBase64
        }
    }

    /** Receive: import tokens from a sender. */
    suspend fun receiveTokens(tokensB64: List<String>) {
        withLoading {
            val updated = bridge.receiveTokens(_state.value, tokensB64)
            _state.value = updated
            persistence.save(updated.json)
        }
    }

    /** Get a base64-encoded receiving ticket for QR display. */
    suspend fun getReceivingTicket(): String {
        return bridge.getReceivingTicketB64(_state.value)
    }

    /** Validate all held tokens. */
    suspend fun validate(): ValidationResult {
        return withLoading {
            val result = bridge.validate(_state.value)
            _state.value = result.state
            persistence.save(result.state.json)
            result
        }
    }

    private suspend fun <T> withLoading(block: suspend () -> T): T {
        _isLoading.value = true
        _error.value = null
        return try {
            block()
        } catch (e: Exception) {
            _error.value = e.message ?: "Unknown error"
            throw e
        } finally {
            _isLoading.value = false
        }
    }
}

/**
 * Platform-specific wallet persistence (SharedPreferences on Android,
 * UserDefaults on iOS, etc.).
 */
interface WalletPersistence {
    suspend fun save(json: String)
    suspend fun load(): String?
    suspend fun clear()
}
