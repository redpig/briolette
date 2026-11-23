package com.briolette.wallet.ui.screens

import androidx.compose.foundation.Image
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.ImageBitmap
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.unit.dp
import com.briolette.wallet.data.WalletRepository
import com.briolette.wallet.ui.components.QrCodeGenerator

/**
 * Displays the user's receiving ticket as a QR code.
 *
 * Other users scan this QR code to send payments. The QR encodes a
 * base64 SignedTicket — the sender uses it as the recipient address
 * in their transfer operation.
 */
@Composable
fun MyQrScreen(
    repository: WalletRepository,
    qrGenerator: QrCodeGenerator,
    onBack: () -> Unit,
) {
    val state by repository.state.collectAsState()
    var ticketB64 by remember { mutableStateOf<String?>(null) }
    var qrBitmap by remember { mutableStateOf<ImageBitmap?>(null) }
    var errorMsg by remember { mutableStateOf<String?>(null) }

    LaunchedEffect(state) {
        try {
            val b64 = repository.getReceivingTicket()
            ticketB64 = b64
            qrBitmap = qrGenerator.generate(b64, 512)
        } catch (e: Exception) {
            errorMsg = e.message ?: "Failed to generate QR code"
        }
    }

    Column(
        modifier = Modifier
            .fillMaxSize()
            .padding(24.dp),
        horizontalAlignment = Alignment.CenterHorizontally,
    ) {
        // Top bar
        Row(
            modifier = Modifier.fillMaxWidth(),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            TextButton(onClick = onBack) {
                Text("Back")
            }
            Spacer(Modifier.weight(1f))
            Text(
                "My Receiving Address",
                style = MaterialTheme.typography.titleMedium,
            )
            Spacer(Modifier.weight(1f))
            // Placeholder for symmetry
            Spacer(Modifier.width(48.dp))
        }

        Spacer(Modifier.height(32.dp))

        Text(
            text = "Scan to send me tokens",
            style = MaterialTheme.typography.bodyLarge,
            color = MaterialTheme.colorScheme.onBackground.copy(alpha = 0.7f),
        )

        Spacer(Modifier.height(24.dp))

        // QR code display
        Card(
            shape = RoundedCornerShape(20.dp),
            colors = CardDefaults.cardColors(
                containerColor = MaterialTheme.colorScheme.surface,
            ),
            elevation = CardDefaults.cardElevation(defaultElevation = 4.dp),
        ) {
            Box(
                modifier = Modifier.padding(24.dp),
                contentAlignment = Alignment.Center,
            ) {
                when {
                    qrBitmap != null -> {
                        Image(
                            bitmap = qrBitmap!!,
                            contentDescription = "Receiving QR Code",
                            modifier = Modifier.size(280.dp),
                        )
                    }
                    errorMsg != null -> {
                        Text(
                            text = errorMsg!!,
                            color = MaterialTheme.colorScheme.error,
                            textAlign = TextAlign.Center,
                            modifier = Modifier.size(280.dp)
                                .wrapContentHeight(Alignment.CenterVertically),
                        )
                    }
                    else -> {
                        CircularProgressIndicator(
                            modifier = Modifier.size(48.dp),
                        )
                    }
                }
            }
        }

        Spacer(Modifier.height(24.dp))

        // Info text
        Card(
            colors = CardDefaults.cardColors(
                containerColor = MaterialTheme.colorScheme.secondaryContainer,
            ),
            modifier = Modifier.fillMaxWidth(),
        ) {
            Text(
                text = "This QR code contains your SignedTicket. " +
                    "Share it with a sender so they can transfer tokens to you. " +
                    "Each ticket is valid for approximately 24 hours.",
                modifier = Modifier.padding(16.dp),
                style = MaterialTheme.typography.bodySmall,
                color = MaterialTheme.colorScheme.onSecondaryContainer,
            )
        }

        Spacer(Modifier.height(16.dp))

        Text(
            text = "${state.ticketCount} tickets remaining",
            style = MaterialTheme.typography.bodyMedium,
            color = MaterialTheme.colorScheme.onBackground.copy(alpha = 0.5f),
        )
    }
}
