package com.briolette.wallet

import androidx.compose.ui.graphics.ImageBitmap
import com.briolette.wallet.data.*
import com.briolette.wallet.ui.components.QrCodeGenerator

/**
 * iOS QR code generator stub.
 *
 * Real implementation would use CoreImage's CIQRCodeGenerator filter
 * via Kotlin/Native interop with the CIFilter API.
 */
class IosQrCodeGenerator : QrCodeGenerator {
    override fun generate(data: String, size: Int): ImageBitmap {
        // TODO: Implement via CIFilter("CIQRCodeGenerator")
        // val filter = CIFilter.filterWithName("CIQRCodeGenerator")
        // filter.setValue(data.toNSData(), "inputMessage")
        // ... convert CIImage -> UIImage -> Compose ImageBitmap
        throw UnsupportedOperationException("iOS QR generation not yet implemented")
    }
}

/**
 * iOS wallet persistence using NSUserDefaults.
 */
class IosWalletPersistence : WalletPersistence {
    // TODO: Use platform.Foundation.NSUserDefaults via Kotlin/Native

    override suspend fun save(json: String) {
        // NSUserDefaults.standardUserDefaults.setObject(json, "briolette_wallet_json")
    }

    override suspend fun load(): String? {
        // return NSUserDefaults.standardUserDefaults.stringForKey("briolette_wallet_json")
        return null
    }

    override suspend fun clear() {
        // NSUserDefaults.standardUserDefaults.removeObjectForKey("briolette_wallet_json")
    }
}

/**
 * iOS WalletBridge — calls Rust FFI via the Swift UniFFI bindings.
 *
 * The Swift bindings are generated from the same UDL file and linked
 * via the static library produced by `cargo build -p briolette-mobile-ffi`.
 */
class IosWalletBridge : WalletBridge {
    override suspend fun createWallet(name: String, config: NetworkConfig): WalletState {
        throw UnsupportedOperationException("iOS UniFFI bindings not yet generated")
    }

    override suspend fun loadWallet(json: String): WalletState {
        throw UnsupportedOperationException("iOS UniFFI bindings not yet generated")
    }

    override suspend fun saveWallet(state: WalletState): String = state.json

    override suspend fun synchronize(state: WalletState): WalletState {
        throw UnsupportedOperationException("iOS UniFFI bindings not yet generated")
    }

    override suspend fun requestTickets(state: WalletState, count: Int): WalletState {
        throw UnsupportedOperationException("iOS UniFFI bindings not yet generated")
    }

    override suspend fun withdraw(state: WalletState, amount: Int): WalletState {
        throw UnsupportedOperationException("iOS UniFFI bindings not yet generated")
    }

    override suspend fun transfer(
        state: WalletState,
        recipientTicketB64: String,
        amount: Int,
    ): TransferResult {
        throw UnsupportedOperationException("iOS UniFFI bindings not yet generated")
    }

    override suspend fun receiveTokens(state: WalletState, tokensB64: List<String>): WalletState {
        throw UnsupportedOperationException("iOS UniFFI bindings not yet generated")
    }

    override suspend fun validate(state: WalletState): ValidationResult {
        throw UnsupportedOperationException("iOS UniFFI bindings not yet generated")
    }

    override suspend fun getReceivingTicketB64(state: WalletState): String {
        throw UnsupportedOperationException("iOS UniFFI bindings not yet generated")
    }
}
