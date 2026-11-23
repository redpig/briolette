package com.briolette.wallet.ui.components

import androidx.compose.ui.graphics.ImageBitmap

/**
 * Platform-specific QR code generation.
 *
 * Android uses ZXing, iOS uses CoreImage CIFilter.
 */
interface QrCodeGenerator {
    /**
     * Generate a QR code bitmap from the given data string.
     *
     * @param data The string to encode in the QR code
     * @param size The width and height of the output bitmap in pixels
     * @return The QR code as a Compose ImageBitmap
     */
    fun generate(data: String, size: Int): ImageBitmap
}

/**
 * Platform-specific QR code scanner.
 *
 * Android uses ML Kit + CameraX, iOS uses AVFoundation.
 * The scanner is implemented as a platform-specific Composable.
 */
interface QrCodeScanner {
    /** Called when a QR code is successfully decoded. */
    var onResult: ((String) -> Unit)?

    /** Called when scanning encounters an error. */
    var onError: ((String) -> Unit)?
}
