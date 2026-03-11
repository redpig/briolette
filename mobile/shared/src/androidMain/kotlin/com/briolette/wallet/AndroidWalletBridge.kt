package com.briolette.wallet

import com.briolette.wallet.data.*
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext
import uniffi.briolette.AttestationData as FfiAttestationData
import uniffi.briolette.KeyInitResult as FfiKeyInitResult
import uniffi.briolette.SplitKeyStep1Result as FfiSplitKeyStep1Result
import uniffi.briolette.SplitKeyStep2aResult as FfiSplitKeyStep2aResult
import uniffi.briolette.SplitKeyStep2bResult as FfiSplitKeyStep2bResult
import uniffi.briolette.Balance as FfiBalance
import uniffi.briolette.TransferResult as FfiTransferResult
import uniffi.briolette.ValidationResult as FfiValidationResult
import uniffi.briolette.WalletState as FfiWalletState

/**
 * Android WalletBridge using real UniFFI Rust bindings.
 *
 * All operations dispatch to [Dispatchers.IO] because the Rust FFI
 * functions block (they create a tokio runtime internally).
 *
 * Translates between Kotlin common data models and UniFFI-generated types.
 */
class AndroidWalletBridge : WalletBridge {

    override suspend fun createWallet(name: String, config: NetworkConfig): WalletState {
        return withContext(Dispatchers.IO) {
            val json = uniffi.briolette.createWallet(
                name,
                config.registrarUri,
                config.clerkUri,
                config.mintUri,
                config.validateUri,
            )
            uniffi.briolette.loadWallet(json).toKotlin()
        }
    }

    override suspend fun createWalletWithAttestation(
        name: String,
        config: NetworkConfig,
        attestation: HwAttestationData,
    ): WalletState {
        return withContext(Dispatchers.IO) {
            val json = uniffi.briolette.createWalletWithAttestation(
                name,
                config.registrarUri,
                config.clerkUri,
                config.mintUri,
                config.validateUri,
                FfiAttestationData(
                    algorithm = attestation.algorithm,
                    signatureB64 = attestation.signatureB64,
                    publicKeyB64 = attestation.publicKeyB64,
                ),
            )
            uniffi.briolette.loadWallet(json).toKotlin()
        }
    }

    override suspend fun initWalletKeys(name: String, config: NetworkConfig): KeyInitResult {
        return withContext(Dispatchers.IO) {
            val result: FfiKeyInitResult = uniffi.briolette.initWalletKeys(
                name,
                config.registrarUri,
                config.clerkUri,
                config.mintUri,
                config.validateUri,
            )
            KeyInitResult(
                walletJson = result.walletJson,
                challengePreimageB64 = result.challengePreimageB64,
                nacCardPublicKeyB64 = result.nacCardPublicKeyB64,
                ttcCardPublicKeyB64 = result.ttcCardPublicKeyB64,
            )
        }
    }

    override suspend fun registerWalletWithAttestation(
        walletJson: String,
        attestation: HwAttestationData,
        nacCardPublicKeyB64: String,
        ttcCardPublicKeyB64: String,
    ): WalletState {
        return withContext(Dispatchers.IO) {
            val json = uniffi.briolette.registerWalletWithAttestation(
                walletJson,
                FfiAttestationData(
                    algorithm = attestation.algorithm,
                    signatureB64 = attestation.signatureB64,
                    publicKeyB64 = attestation.publicKeyB64,
                ),
                nacCardPublicKeyB64,
                ttcCardPublicKeyB64,
            )
            uniffi.briolette.loadWallet(json).toKotlin()
        }
    }

    override suspend fun splitKeyStart(name: String, config: NetworkConfig): SplitKeyStep1Result {
        return withContext(Dispatchers.IO) {
            val result: FfiSplitKeyStep1Result = uniffi.briolette.splitKeyStart(
                name, config.registrarUri, config.clerkUri, config.mintUri, config.validateUri,
            )
            SplitKeyStep1Result(stateJson = result.stateJson, bTtcB64 = result.bTtcB64)
        }
    }

    override suspend fun splitKeyAfterTtcCommit(
        stateJson: String, qCardTtcB64: String, uCardTtcB64: String,
    ): SplitKeyStep2aResult {
        return withContext(Dispatchers.IO) {
            val result: FfiSplitKeyStep2aResult = uniffi.briolette.splitKeyAfterTtcCommit(
                stateJson, qCardTtcB64, uCardTtcB64,
            )
            SplitKeyStep2aResult(
                stateJson = result.stateJson,
                cTtcB64 = result.cTtcB64,
                bNacB64 = result.bNacB64,
            )
        }
    }

    override suspend fun splitKeyAfterNacCommit(
        stateJson: String, qCardNacB64: String, uCardNacB64: String,
    ): SplitKeyStep2bResult {
        return withContext(Dispatchers.IO) {
            val result: FfiSplitKeyStep2bResult = uniffi.briolette.splitKeyAfterNacCommit(
                stateJson, qCardNacB64, uCardNacB64,
            )
            SplitKeyStep2bResult(stateJson = result.stateJson, cNacB64 = result.cNacB64)
        }
    }

    override suspend fun splitKeyComplete(
        stateJson: String, sCardTtcB64: String, sCardNacB64: String,
    ): KeyInitResult {
        return withContext(Dispatchers.IO) {
            val result: FfiKeyInitResult = uniffi.briolette.splitKeyComplete(
                stateJson, sCardTtcB64, sCardNacB64,
            )
            KeyInitResult(
                walletJson = result.walletJson,
                challengePreimageB64 = result.challengePreimageB64,
                nacCardPublicKeyB64 = result.nacCardPublicKeyB64,
                ttcCardPublicKeyB64 = result.ttcCardPublicKeyB64,
            )
        }
    }

    override suspend fun loadWallet(json: String): WalletState {
        return withContext(Dispatchers.IO) {
            uniffi.briolette.loadWallet(json).toKotlin()
        }
    }

    override suspend fun saveWallet(state: WalletState): String {
        return withContext(Dispatchers.IO) {
            uniffi.briolette.saveWallet(state.toFfi())
        }
    }

    override suspend fun synchronize(state: WalletState): WalletState {
        return withContext(Dispatchers.IO) {
            uniffi.briolette.synchronize(state.toFfi(), "").toKotlin()
        }
    }

    override suspend fun requestTickets(state: WalletState, count: Int): WalletState {
        return withContext(Dispatchers.IO) {
            uniffi.briolette.requestTickets(state.toFfi(), "", count.toUInt()).toKotlin()
        }
    }

    override suspend fun withdraw(state: WalletState, amount: Int): WalletState {
        return withContext(Dispatchers.IO) {
            uniffi.briolette.withdraw(state.toFfi(), "", amount.toUInt()).toKotlin()
        }
    }

    override suspend fun transfer(
        state: WalletState,
        recipientTicketB64: String,
        amount: Int,
    ): TransferResult {
        return withContext(Dispatchers.IO) {
            uniffi.briolette.transferTokens(
                state.toFfi(),
                recipientTicketB64,
                amount.toUInt(),
            ).toKotlin()
        }
    }

    override suspend fun receiveTokens(state: WalletState, tokensB64: List<String>): WalletState {
        return withContext(Dispatchers.IO) {
            uniffi.briolette.receiveTokens(state.toFfi(), tokensB64).toKotlin()
        }
    }

    override suspend fun validate(state: WalletState): ValidationResult {
        return withContext(Dispatchers.IO) {
            uniffi.briolette.validateTokens(state.toFfi(), "").toKotlin()
        }
    }

    override suspend fun getReceivingTicketB64(state: WalletState): String {
        return withContext(Dispatchers.IO) {
            uniffi.briolette.getReceivingTicketB64(state.toFfi())
        }
    }
}

// ── FFI ↔ Kotlin conversions ────────────────────────────────────────────

private fun FfiBalance.toKotlin() = Balance(
    whole = this.whole,
    fractional = this.fractional,
    currency = this.currency,
    tokenCount = this.tokenCount.toInt(),
)

private fun FfiWalletState.toKotlin() = WalletState(
    json = this.json,
    balance = this.balance.toKotlin(),
    ticketCount = this.ticketCount.toInt(),
    walletName = this.walletName,
)

private fun FfiTransferResult.toKotlin() = TransferResult(
    state = this.state.toKotlin(),
    tokensBase64 = this.tokensB64,
)

private fun FfiValidationResult.toKotlin() = ValidationResult(
    state = this.state.toKotlin(),
    allValid = this.allValid,
    validCount = this.validCount.toInt(),
    invalidCount = this.invalidCount.toInt(),
)

private fun Balance.toFfi() = FfiBalance(
    whole = this.whole,
    fractional = this.fractional,
    currency = this.currency,
    tokenCount = this.tokenCount.toUInt(),
)

private fun WalletState.toFfi() = FfiWalletState(
    json = this.json,
    balance = this.balance.toFfi(),
    ticketCount = this.ticketCount.toUInt(),
    walletName = this.walletName,
)
