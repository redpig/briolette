package com.briolette.wallet.ui.screens

import androidx.compose.foundation.Image
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.ImageBitmap
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.unit.dp
import com.briolette.wallet.ui.components.QrCodeGenerator

/**
 * Displays the signed tokens as QR codes for the recipient to scan.
 *
 * After a payment, the sender shows this screen to the recipient.
 * Each token is encoded as a separate QR code (tokens can be large,
 * so splitting across multiple QR codes keeps each one scannable).
 *
 * The QR payload is a JSON envelope:
 *   {"type":"transfer","tokens":["<base64>","<base64>",...]}
 *
 * For single tokens that fit in one QR, we use the simpler format:
 *   {"type":"token","data":"<base64>"}
 */
@Composable
fun TransferQrScreen(
    tokensBase64: List<String>,
    qrGenerator: QrCodeGenerator,
    onDone: () -> Unit,
) {
    val qrBitmaps = remember(tokensBase64) {
        if (tokensBase64.size == 1) {
            // Single token — encode as simple payload
            val payload = """{"type":"token","data":"${tokensBase64[0]}"}"""
            listOf(qrGenerator.generate(payload, 512))
        } else {
            // Multiple tokens — encode as transfer envelope
            val json = tokensBase64.joinToString(",") { "\"$it\"" }
            val payload = """{"type":"transfer","tokens":[$json]}"""
            // If payload fits in a QR code (~4000 chars), use single QR
            if (payload.length < 3500) {
                listOf(qrGenerator.generate(payload, 512))
            } else {
                // Split into individual QR codes
                tokensBase64.mapIndexed { i, b64 ->
                    val p = """{"type":"token","data":"$b64","index":$i,"total":${tokensBase64.size}}"""
                    qrGenerator.generate(p, 400)
                }
            }
        }
    }

    var currentIndex by remember { mutableStateOf(0) }

    Column(
        modifier = Modifier
            .fillMaxSize()
            .padding(24.dp),
        horizontalAlignment = Alignment.CenterHorizontally,
    ) {
        Text(
            "Payment Sent",
            style = MaterialTheme.typography.headlineSmall,
        )
        Spacer(Modifier.height(8.dp))
        Text(
            "Show this QR code to the recipient",
            style = MaterialTheme.typography.bodyMedium,
            color = MaterialTheme.colorScheme.onBackground.copy(alpha = 0.7f),
        )

        Spacer(Modifier.height(24.dp))

        // QR display
        Card(
            shape = RoundedCornerShape(20.dp),
            colors = CardDefaults.cardColors(containerColor = MaterialTheme.colorScheme.surface),
            elevation = CardDefaults.cardElevation(defaultElevation = 4.dp),
        ) {
            Column(
                modifier = Modifier.padding(24.dp),
                horizontalAlignment = Alignment.CenterHorizontally,
            ) {
                if (currentIndex < qrBitmaps.size) {
                    Image(
                        bitmap = qrBitmaps[currentIndex],
                        contentDescription = "Transfer QR Code",
                        modifier = Modifier.size(280.dp),
                    )
                }

                if (qrBitmaps.size > 1) {
                    Spacer(Modifier.height(12.dp))
                    Text(
                        "Token ${currentIndex + 1} of ${qrBitmaps.size}",
                        style = MaterialTheme.typography.bodyMedium,
                    )
                    Spacer(Modifier.height(8.dp))
                    Row(
                        horizontalArrangement = Arrangement.spacedBy(12.dp),
                    ) {
                        OutlinedButton(
                            onClick = { if (currentIndex > 0) currentIndex-- },
                            enabled = currentIndex > 0,
                        ) {
                            Text("Prev")
                        }
                        OutlinedButton(
                            onClick = { if (currentIndex < qrBitmaps.size - 1) currentIndex++ },
                            enabled = currentIndex < qrBitmaps.size - 1,
                        ) {
                            Text("Next")
                        }
                    }
                }
            }
        }

        Spacer(Modifier.height(16.dp))

        Card(
            colors = CardDefaults.cardColors(
                containerColor = MaterialTheme.colorScheme.tertiaryContainer,
            ),
            modifier = Modifier.fillMaxWidth(),
        ) {
            Text(
                text = "The recipient should scan this QR code using " +
                    "the \"Receive\" function in their Briolette wallet. " +
                    "Tokens are cryptographically signed to your recipient's " +
                    "ticket and cannot be intercepted.",
                modifier = Modifier.padding(16.dp),
                style = MaterialTheme.typography.bodySmall,
                color = MaterialTheme.colorScheme.onTertiaryContainer,
            )
        }

        Spacer(Modifier.weight(1f))

        Button(
            onClick = onDone,
            modifier = Modifier.fillMaxWidth().height(56.dp),
            shape = RoundedCornerShape(16.dp),
        ) {
            Text("Done")
        }
    }
}
