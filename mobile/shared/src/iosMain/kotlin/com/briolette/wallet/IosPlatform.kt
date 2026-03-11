package com.briolette.wallet

import androidx.compose.ui.graphics.ImageBitmap
import androidx.compose.ui.graphics.toComposeImageBitmap
import com.briolette.wallet.data.*
import com.briolette.wallet.ui.components.QrCodeGenerator
import kotlinx.cinterop.ExperimentalForeignApi
import kotlinx.cinterop.addressOf
import kotlinx.cinterop.usePinned
import org.jetbrains.skia.ColorAlphaType
import org.jetbrains.skia.ColorType
import org.jetbrains.skia.ImageInfo
import platform.CoreGraphics.*
import platform.CoreImage.CIContext
import platform.CoreImage.CIFilter
import platform.CoreImage.filterWithName
import platform.Foundation.NSString
import platform.Foundation.NSUserDefaults
import platform.Foundation.NSUTF8StringEncoding
import platform.Foundation.dataUsingEncoding
import platform.Foundation.setValue

/**
 * iOS QR code generator using CoreImage CIQRCodeGenerator filter.
 */
class IosQrCodeGenerator : QrCodeGenerator {
    @OptIn(ExperimentalForeignApi::class)
    override fun generate(data: String, size: Int): ImageBitmap {
        val filter = CIFilter.filterWithName("CIQRCodeGenerator")
            ?: throw IllegalStateException("CIQRCodeGenerator filter not available")

        val inputData = (data as NSString).dataUsingEncoding(NSUTF8StringEncoding)
            ?: throw IllegalArgumentException("Failed to encode data")

        filter.setValue(inputData, forKey = "inputMessage")
        filter.setValue("M", forKey = "inputCorrectionLevel")

        val outputImage = filter.outputImage
            ?: throw IllegalStateException("CIFilter produced no output")

        // Scale up to requested size
        val extent = outputImage.extent
        val scaleX = size.toDouble() / extent.size.width
        val scaleY = size.toDouble() / extent.size.height
        val scaledImage = outputImage.imageByApplyingTransform(
            CGAffineTransformMakeScale(scaleX, scaleY)
        )

        // Render to CGImage
        val context = CIContext()
        val cgImage = context.createCGImage(scaledImage, scaledImage.extent)
            ?: throw IllegalStateException("Failed to create CGImage")

        // Convert CGImage to pixel data
        val width = CGImageGetWidth(cgImage).toInt()
        val height = CGImageGetHeight(cgImage).toInt()
        val bytesPerRow = width * 4
        val pixels = ByteArray(height * bytesPerRow)

        val colorSpace = CGColorSpaceCreateDeviceRGB()
        val bitmapContext = CGBitmapContextCreate(
            data = pixels.usePinned { it.addressOf(0) },
            width = width.toULong(),
            height = height.toULong(),
            bitsPerComponent = 8u,
            bytesPerRow = bytesPerRow.toULong(),
            space = colorSpace,
            bitmapInfo = CGImageAlphaInfo.kCGImageAlphaPremultipliedLast.value,
        )

        CGContextDrawImage(bitmapContext, CGRectMake(0.0, 0.0, width.toDouble(), height.toDouble()), cgImage)
        CGContextRelease(bitmapContext)

        // Create Skia Image from pixel data
        val imageInfo = ImageInfo(width, height, ColorType.RGBA_8888, ColorAlphaType.PREMUL)
        val skiaImage = org.jetbrains.skia.Image.makeRaster(imageInfo, pixels, bytesPerRow)
        return skiaImage.toComposeImageBitmap()
    }
}

/**
 * iOS wallet persistence using NSUserDefaults.
 */
class IosWalletPersistence : WalletPersistence {
    private val defaults = NSUserDefaults.standardUserDefaults
    private val key = "briolette_wallet_json"

    override suspend fun save(json: String) {
        defaults.setObject(json, forKey = key)
        defaults.synchronize()
    }

    override suspend fun load(): String? {
        return defaults.stringForKey(key)
    }

    override suspend fun clear() {
        defaults.removeObjectForKey(key)
        defaults.synchronize()
    }
}

/**
 * Delegate protocol for Swift to provide wallet FFI operations.
 *
 * The Swift app implements this interface using the UniFFI-generated bindings
 * and sets it on [IosWalletBridge] at startup via [IosWalletBridge.setDelegate].
 */
/**
 * iOS attestation provider using DCAppAttestService via the Swift delegate.
 *
 * On iOS 14+, this calls the Swift-side App Attest helper. On older versions,
 * returns null (falls back to Algorithm::NONE).
 */
class IosAppAttestProvider : HwAttestationProvider {
    override val isSupported: Boolean
        get() = true  // Actual check happens in Swift at generation time

    override suspend fun generate(challengePreimageB64: String): HwAttestationData? {
        val delegate = IosWalletBridge.getDelegate() ?: return null
        return try {
            // Pass the base64 preimage to Swift; it will decode, SHA-256 hash,
            // and use as the App Attest challenge.
            val result = delegate.generateAttestationWithPreimage(challengePreimageB64)
            val algo = (result["algorithm"] as? Number)?.toInt() ?: return null
            val sigB64 = result["signatureB64"] as? String ?: return null
            val pkB64 = result["publicKeyB64"] as? String ?: return null
            HwAttestationData(
                algorithm = algo,
                signatureB64 = sigB64,
                publicKeyB64 = pkB64,
            )
        } catch (e: Exception) {
            null
        }
    }
}

interface IosWalletDelegate {
    fun createWallet(name: String, registrarUri: String, clerkUri: String, mintUri: String, validateUri: String): String
    fun createWalletWithAttestation(
        name: String,
        registrarUri: String,
        clerkUri: String,
        mintUri: String,
        validateUri: String,
        algorithm: Int,
        signatureB64: String,
        publicKeyB64: String,
    ): String {
        // Default: fall back to non-attested for backward compatibility.
        return createWallet(name, registrarUri, clerkUri, mintUri, validateUri)
    }
    fun loadWallet(json: String): Map<String, Any?>
    fun saveWallet(stateJson: String): String
    fun synchronize(stateJson: String, clerkUri: String): Map<String, Any?>
    fun requestTickets(stateJson: String, clerkUri: String, count: Int): Map<String, Any?>
    fun withdraw(stateJson: String, mintUri: String, amount: Int): Map<String, Any?>
    fun transferTokens(stateJson: String, recipientTicketB64: String, amount: Int): Map<String, Any?>
    fun receiveTokens(stateJson: String, tokensB64: List<String>): Map<String, Any?>
    fun validateTokens(stateJson: String, validateUri: String): Map<String, Any?>
    fun getReceivingTicketB64(stateJson: String): String
    fun getBalance(stateJson: String): Map<String, Any?>
    fun getTicketCount(stateJson: String): Int
    fun generateAttestationWithPreimage(preimageB64: String): Map<String, Any?> {
        // Default: return empty, meaning attestation not supported.
        return emptyMap()
    }
    fun initWalletKeys(
        name: String,
        registrarUri: String,
        clerkUri: String,
        mintUri: String,
        validateUri: String,
    ): Map<String, Any?> {
        return emptyMap()
    }
    fun registerWalletWithAttestation(
        walletJson: String,
        algorithm: Int,
        signatureB64: String,
        publicKeyB64: String,
    ): String {
        return "{}"
    }
}

/**
 * iOS WalletBridge that delegates to a Swift-provided [IosWalletDelegate].
 *
 * If no delegate is set, operations fall back to JSON-only mode for basic
 * wallet state management without network operations.
 */
class IosWalletBridge : WalletBridge {

    companion object {
        private var delegate: IosWalletDelegate? = null

        /**
         * Set the Swift-side delegate. Call this from Swift before
         * creating the Compose UI.
         */
        fun setDelegate(delegate: IosWalletDelegate) {
            this.delegate = delegate
        }

        fun getDelegate(): IosWalletDelegate? = delegate
    }

    private fun requireDelegate(): IosWalletDelegate {
        return delegate ?: throw UnsupportedOperationException(
            "iOS wallet delegate not set. Call IosWalletBridge.setDelegate() from Swift at startup."
        )
    }

    override suspend fun createWallet(name: String, config: NetworkConfig): WalletState {
        val d = requireDelegate()
        val json = d.createWallet(name, config.registrarUri, config.clerkUri, config.mintUri, config.validateUri)
        val stateMap = d.loadWallet(json)
        return mapToWalletState(stateMap, json)
    }

    override suspend fun createWalletWithAttestation(
        name: String,
        config: NetworkConfig,
        attestation: HwAttestationData,
    ): WalletState {
        val d = requireDelegate()
        val json = d.createWalletWithAttestation(
            name,
            config.registrarUri,
            config.clerkUri,
            config.mintUri,
            config.validateUri,
            attestation.algorithm,
            attestation.signatureB64,
            attestation.publicKeyB64,
        )
        val stateMap = d.loadWallet(json)
        return mapToWalletState(stateMap, json)
    }

    override suspend fun initWalletKeys(name: String, config: NetworkConfig): KeyInitResult {
        val d = requireDelegate()
        val result = d.initWalletKeys(
            name, config.registrarUri, config.clerkUri, config.mintUri, config.validateUri,
        )
        return KeyInitResult(
            walletJson = result["walletJson"] as? String ?: "{}",
            challengePreimageB64 = result["challengePreimageB64"] as? String ?: "",
        )
    }

    override suspend fun registerWalletWithAttestation(
        walletJson: String,
        attestation: HwAttestationData,
    ): WalletState {
        val d = requireDelegate()
        val json = d.registerWalletWithAttestation(
            walletJson,
            attestation.algorithm,
            attestation.signatureB64,
            attestation.publicKeyB64,
        )
        val stateMap = d.loadWallet(json)
        return mapToWalletState(stateMap, json)
    }

    override suspend fun loadWallet(json: String): WalletState {
        val d = requireDelegate()
        val stateMap = d.loadWallet(json)
        return mapToWalletState(stateMap, json)
    }

    override suspend fun saveWallet(state: WalletState): String {
        val d = requireDelegate()
        return d.saveWallet(state.json)
    }

    override suspend fun synchronize(state: WalletState): WalletState {
        val d = requireDelegate()
        val result = d.synchronize(state.json, "")
        return mapToWalletState(result, result["json"] as? String ?: state.json)
    }

    override suspend fun requestTickets(state: WalletState, count: Int): WalletState {
        val d = requireDelegate()
        val result = d.requestTickets(state.json, "", count)
        return mapToWalletState(result, result["json"] as? String ?: state.json)
    }

    override suspend fun withdraw(state: WalletState, amount: Int): WalletState {
        val d = requireDelegate()
        val result = d.withdraw(state.json, "", amount)
        return mapToWalletState(result, result["json"] as? String ?: state.json)
    }

    override suspend fun transfer(
        state: WalletState,
        recipientTicketB64: String,
        amount: Int,
    ): TransferResult {
        val d = requireDelegate()
        val result = d.transferTokens(state.json, recipientTicketB64, amount)
        @Suppress("UNCHECKED_CAST")
        val stateMap = result["state"] as? Map<String, Any?> ?: emptyMap()
        @Suppress("UNCHECKED_CAST")
        val tokensB64 = result["tokensB64"] as? List<String> ?: emptyList()
        return TransferResult(
            state = mapToWalletState(stateMap, stateMap["json"] as? String ?: state.json),
            tokensBase64 = tokensB64,
        )
    }

    override suspend fun receiveTokens(state: WalletState, tokensB64: List<String>): WalletState {
        val d = requireDelegate()
        val result = d.receiveTokens(state.json, tokensB64)
        return mapToWalletState(result, result["json"] as? String ?: state.json)
    }

    override suspend fun validate(state: WalletState): ValidationResult {
        val d = requireDelegate()
        val result = d.validateTokens(state.json, "")
        @Suppress("UNCHECKED_CAST")
        val stateMap = result["state"] as? Map<String, Any?> ?: emptyMap()
        return ValidationResult(
            state = mapToWalletState(stateMap, stateMap["json"] as? String ?: state.json),
            allValid = result["allValid"] as? Boolean ?: false,
            validCount = (result["validCount"] as? Number)?.toInt() ?: 0,
            invalidCount = (result["invalidCount"] as? Number)?.toInt() ?: 0,
        )
    }

    override suspend fun getReceivingTicketB64(state: WalletState): String {
        val d = requireDelegate()
        return d.getReceivingTicketB64(state.json)
    }

    private fun mapToWalletState(map: Map<String, Any?>, json: String): WalletState {
        return WalletState(
            json = json,
            balance = Balance(
                whole = (map["whole"] as? Number)?.toInt() ?: 0,
                fractional = (map["fractional"] as? Number)?.toInt() ?: 0,
                currency = map["currency"] as? String ?: "TEST",
                tokenCount = (map["tokenCount"] as? Number)?.toInt() ?: 0,
            ),
            ticketCount = (map["ticketCount"] as? Number)?.toInt() ?: 0,
            walletName = map["walletName"] as? String ?: "unknown",
        )
    }
}
