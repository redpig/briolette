package com.briolette.wallet

import com.briolette.wallet.data.*
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext
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
