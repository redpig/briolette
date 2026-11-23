package com.briolette.wallet.ui.screens

import androidx.compose.foundation.layout.*
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.unit.dp
import com.briolette.wallet.data.WalletRepository
import kotlinx.coroutines.launch

/**
 * Receive screen — scan a sender's transfer QR code to import tokens.
 *
 * Flow:
 * 1. User shows their QR code (MyQrScreen) to the sender
 * 2. Sender creates a transfer and shows a transfer QR
 * 3. User scans the transfer QR on THIS screen to import the tokens
 */
@Composable
fun ReceiveScreen(
    repository: WalletRepository,
    onScanQr: () -> Unit,
    scannedTokensB64: List<String>?,
    onBack: () -> Unit,
) {
    val state by repository.state.collectAsState()
    val isLoading by repository.isLoading.collectAsState()
    val scope = rememberCoroutineScope()

    var received by remember { mutableStateOf(false) }
    var errorMsg by remember { mutableStateOf<String?>(null) }

    // Auto-import when tokens are scanned
    LaunchedEffect(scannedTokensB64) {
        if (scannedTokensB64 != null && scannedTokensB64.isNotEmpty() && !received) {
            try {
                repository.receiveTokens(scannedTokensB64)
                received = true
            } catch (e: Exception) {
                errorMsg = e.message ?: "Failed to import tokens"
            }
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
            Text("Receive Tokens", style = MaterialTheme.typography.titleMedium)
            Spacer(Modifier.weight(1f))
            Spacer(Modifier.width(48.dp))
        }

        Spacer(Modifier.height(48.dp))

        if (received) {
            // Success state
            Card(
                modifier = Modifier.fillMaxWidth(),
                colors = CardDefaults.cardColors(
                    containerColor = MaterialTheme.colorScheme.primaryContainer,
                ),
                shape = RoundedCornerShape(20.dp),
            ) {
                Column(
                    modifier = Modifier
                        .fillMaxWidth()
                        .padding(32.dp),
                    horizontalAlignment = Alignment.CenterHorizontally,
                ) {
                    Text(
                        text = "Tokens Received!",
                        style = MaterialTheme.typography.headlineMedium,
                        color = MaterialTheme.colorScheme.onPrimaryContainer,
                    )
                    Spacer(Modifier.height(16.dp))
                    Text(
                        text = "New balance: ${state.balance.displayAmount} ${state.balance.currency}",
                        style = MaterialTheme.typography.bodyLarge,
                        color = MaterialTheme.colorScheme.onPrimaryContainer,
                    )
                    Spacer(Modifier.height(24.dp))
                    OutlinedButton(onClick = {
                        scope.launch {
                            try {
                                repository.validate()
                            } catch (_: Exception) {}
                        }
                    }) {
                        Text("Validate Tokens")
                    }
                }
            }

            Spacer(Modifier.height(24.dp))

            Button(
                onClick = onBack,
                modifier = Modifier.fillMaxWidth().height(56.dp),
                shape = RoundedCornerShape(16.dp),
            ) {
                Text("Done")
            }
        } else {
            // Waiting to scan
            Text(
                text = "Step 1: Share your QR code with the sender",
                style = MaterialTheme.typography.bodyLarge,
            )
            Spacer(Modifier.height(8.dp))
            Text(
                text = "Step 2: After they transfer, scan their QR code",
                style = MaterialTheme.typography.bodyLarge,
            )

            Spacer(Modifier.height(32.dp))

            Button(
                onClick = onScanQr,
                enabled = !isLoading,
                modifier = Modifier
                    .fillMaxWidth()
                    .height(72.dp),
                shape = RoundedCornerShape(16.dp),
            ) {
                if (isLoading) {
                    CircularProgressIndicator(
                        modifier = Modifier.size(24.dp),
                        color = MaterialTheme.colorScheme.onPrimary,
                    )
                } else {
                    Text("Scan Transfer QR Code")
                }
            }

            errorMsg?.let { msg ->
                Spacer(Modifier.height(16.dp))
                Text(
                    text = msg,
                    color = MaterialTheme.colorScheme.error,
                    style = MaterialTheme.typography.bodyMedium,
                )
            }
        }
    }
}
