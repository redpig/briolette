package com.briolette.wallet

import com.briolette.wallet.data.*
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext

/**
 * Android WalletBridge implementation that calls the Rust UniFFI bindings.
 *
 * All operations are dispatched to [Dispatchers.IO] since the Rust FFI
 * functions are blocking (they run a tokio runtime internally).
 *
 * NOTE: This currently uses a stub implementation. When the UniFFI bindings
 * are generated (via `cargo run --bin uniffi-bindgen ...`), replace the stub
 * calls with actual `briolette_mobile.*` FFI calls.
 */
class AndroidWalletBridge : WalletBridge {

    override suspend fun createWallet(name: String, config: NetworkConfig): WalletState {
        return withContext(Dispatchers.IO) {
            // TODO: Replace with UniFFI call:
            // val json = briolette_mobile.createWallet(
            //     name, config.registrarUri, config.clerkUri,
            //     config.mintUri, config.validateUri
            // )
            // val ffiState = briolette_mobile.loadWallet(json)
            // ffiState.toKotlin()
            throw UnsupportedOperationException(
                "UniFFI bindings not yet generated. " +
                "Run: cargo run -p briolette-mobile-ffi --bin uniffi-bindgen " +
                "generate src/mobile-ffi/src/briolette.udl --language kotlin"
            )
        }
    }

    override suspend fun loadWallet(json: String): WalletState {
        return withContext(Dispatchers.IO) {
            // val ffiState = briolette_mobile.loadWallet(json)
            // ffiState.toKotlin()
            throw UnsupportedOperationException("UniFFI bindings not yet generated")
        }
    }

    override suspend fun saveWallet(state: WalletState): String {
        return withContext(Dispatchers.IO) {
            // briolette_mobile.saveWallet(state.toFfi())
            state.json
        }
    }

    override suspend fun synchronize(state: WalletState): WalletState {
        return withContext(Dispatchers.IO) {
            // val ffiState = briolette_mobile.synchronize(state.toFfi(), "")
            // ffiState.toKotlin()
            throw UnsupportedOperationException("UniFFI bindings not yet generated")
        }
    }

    override suspend fun requestTickets(state: WalletState, count: Int): WalletState {
        return withContext(Dispatchers.IO) {
            // val ffiState = briolette_mobile.requestTickets(state.toFfi(), "", count.toUInt())
            // ffiState.toKotlin()
            throw UnsupportedOperationException("UniFFI bindings not yet generated")
        }
    }

    override suspend fun withdraw(state: WalletState, amount: Int): WalletState {
        return withContext(Dispatchers.IO) {
            // val ffiState = briolette_mobile.withdraw(state.toFfi(), "", amount.toUInt())
            // ffiState.toKotlin()
            throw UnsupportedOperationException("UniFFI bindings not yet generated")
        }
    }

    override suspend fun transfer(
        state: WalletState,
        recipientTicketB64: String,
        amount: Int,
    ): TransferResult {
        return withContext(Dispatchers.IO) {
            // val ffiResult = briolette_mobile.transferTokens(
            //     state.toFfi(), recipientTicketB64, amount.toUInt()
            // )
            // ffiResult.toKotlin()
            throw UnsupportedOperationException("UniFFI bindings not yet generated")
        }
    }

    override suspend fun receiveTokens(state: WalletState, tokensB64: List<String>): WalletState {
        return withContext(Dispatchers.IO) {
            // val ffiState = briolette_mobile.receiveTokens(state.toFfi(), tokensB64)
            // ffiState.toKotlin()
            throw UnsupportedOperationException("UniFFI bindings not yet generated")
        }
    }

    override suspend fun validate(state: WalletState): ValidationResult {
        return withContext(Dispatchers.IO) {
            // val ffiResult = briolette_mobile.validateTokens(state.toFfi(), "")
            // ffiResult.toKotlin()
            throw UnsupportedOperationException("UniFFI bindings not yet generated")
        }
    }

    override suspend fun getReceivingTicketB64(state: WalletState): String {
        return withContext(Dispatchers.IO) {
            // briolette_mobile.getReceivingTicketB64(state.toFfi())
            throw UnsupportedOperationException("UniFFI bindings not yet generated")
        }
    }
}
