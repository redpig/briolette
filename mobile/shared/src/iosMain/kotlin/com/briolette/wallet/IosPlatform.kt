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
import platform.CoreFoundation.CFDataGetBytePtr
import platform.CoreFoundation.CFDataGetLength
import platform.CoreGraphics.*
import platform.CoreImage.CIContext
import platform.CoreImage.CIFilter
import platform.CoreImage.filterWithName
import platform.Foundation.NSData
import platform.Foundation.NSString
import platform.Foundation.NSUserDefaults
import platform.Foundation.NSUTF8StringEncoding
import platform.Foundation.create
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
 * iOS WalletBridge — calls Rust FFI via the Swift UniFFI bindings.
 *
 * The Swift bindings are generated from the same UDL file and linked
 * via the static library produced by `cargo build -p briolette-mobile-ffi`.
 */
class IosWalletBridge : WalletBridge {
    override suspend fun createWallet(name: String, config: NetworkConfig): WalletState {
        throw UnsupportedOperationException("iOS UniFFI bindings not yet wired")
    }

    override suspend fun loadWallet(json: String): WalletState {
        throw UnsupportedOperationException("iOS UniFFI bindings not yet wired")
    }

    override suspend fun saveWallet(state: WalletState): String = state.json

    override suspend fun synchronize(state: WalletState): WalletState {
        throw UnsupportedOperationException("iOS UniFFI bindings not yet wired")
    }

    override suspend fun requestTickets(state: WalletState, count: Int): WalletState {
        throw UnsupportedOperationException("iOS UniFFI bindings not yet wired")
    }

    override suspend fun withdraw(state: WalletState, amount: Int): WalletState {
        throw UnsupportedOperationException("iOS UniFFI bindings not yet wired")
    }

    override suspend fun transfer(
        state: WalletState,
        recipientTicketB64: String,
        amount: Int,
    ): TransferResult {
        throw UnsupportedOperationException("iOS UniFFI bindings not yet wired")
    }

    override suspend fun receiveTokens(state: WalletState, tokensB64: List<String>): WalletState {
        throw UnsupportedOperationException("iOS UniFFI bindings not yet wired")
    }

    override suspend fun validate(state: WalletState): ValidationResult {
        throw UnsupportedOperationException("iOS UniFFI bindings not yet wired")
    }

    override suspend fun getReceivingTicketB64(state: WalletState): String {
        throw UnsupportedOperationException("iOS UniFFI bindings not yet wired")
    }
}
