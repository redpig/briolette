package com.briolette.wallet.ui.screens

import androidx.compose.foundation.layout.*
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.text.input.KeyboardType
import androidx.compose.ui.unit.dp
import com.briolette.wallet.data.WalletRepository
import kotlinx.coroutines.launch

/**
 * Payment screen — scan a recipient's QR code and send tokens.
 *
 * Flow:
 * 1. User enters amount
 * 2. User scans recipient's QR code (which contains their SignedTicket)
 * 3. App transfers tokens and shows the result QR for delivery
 */
@Composable
fun PayScreen(
    repository: WalletRepository,
    onScanQr: () -> Unit,
    scannedTicketB64: String?,
    onBack: () -> Unit,
    onShowTransferQr: (List<String>) -> Unit,
) {
    val state by repository.state.collectAsState()
    val isLoading by repository.isLoading.collectAsState()
    val scope = rememberCoroutineScope()

    var amount by remember { mutableStateOf("") }
    var errorMsg by remember { mutableStateOf<String?>(null) }
    var success by remember { mutableStateOf(false) }

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
            Text("Send Payment", style = MaterialTheme.typography.titleMedium)
            Spacer(Modifier.weight(1f))
            Spacer(Modifier.width(48.dp))
        }

        Spacer(Modifier.height(32.dp))

        // Available balance
        Text(
            text = "Available: ${state.balance.displayAmount} ${state.balance.currency}",
            style = MaterialTheme.typography.bodyLarge,
            color = MaterialTheme.colorScheme.onBackground.copy(alpha = 0.7f),
        )

        Spacer(Modifier.height(24.dp))

        // Amount input
        OutlinedTextField(
            value = amount,
            onValueChange = { amount = it.filter { c -> c.isDigit() } },
            label = { Text("Amount") },
            keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Number),
            singleLine = true,
            modifier = Modifier.fillMaxWidth(),
            shape = RoundedCornerShape(12.dp),
        )

        Spacer(Modifier.height(16.dp))

        // Recipient status
        Card(
            modifier = Modifier.fillMaxWidth(),
            colors = CardDefaults.cardColors(
                containerColor = if (scannedTicketB64 != null)
                    MaterialTheme.colorScheme.primaryContainer
                else
                    MaterialTheme.colorScheme.surfaceVariant,
            ),
        ) {
            Row(
                modifier = Modifier
                    .fillMaxWidth()
                    .padding(16.dp),
                verticalAlignment = Alignment.CenterVertically,
            ) {
                Column(modifier = Modifier.weight(1f)) {
                    Text(
                        text = if (scannedTicketB64 != null) "Recipient scanned"
                        else "No recipient yet",
                        style = MaterialTheme.typography.bodyLarge,
                    )
                    if (scannedTicketB64 != null) {
                        Text(
                            text = scannedTicketB64.take(20) + "...",
                            style = MaterialTheme.typography.bodySmall,
                            color = MaterialTheme.colorScheme.onSurface.copy(alpha = 0.5f),
                        )
                    }
                }
                Button(
                    onClick = onScanQr,
                    enabled = !isLoading,
                ) {
                    Text("Scan")
                }
            }
        }

        Spacer(Modifier.height(32.dp))

        // Send button
        Button(
            onClick = {
                val parsedAmount = amount.toIntOrNull()
                if (parsedAmount == null || parsedAmount <= 0) {
                    errorMsg = "Enter a valid amount"
                    return@Button
                }
                if (scannedTicketB64 == null) {
                    errorMsg = "Scan recipient QR code first"
                    return@Button
                }
                if (parsedAmount > state.balance.whole) {
                    errorMsg = "Insufficient funds"
                    return@Button
                }
                errorMsg = null
                scope.launch {
                    try {
                        val tokens = repository.pay(scannedTicketB64, parsedAmount)
                        success = true
                        onShowTransferQr(tokens)
                    } catch (e: Exception) {
                        errorMsg = e.message ?: "Transfer failed"
                    }
                }
            },
            enabled = !isLoading && amount.isNotBlank() && scannedTicketB64 != null,
            modifier = Modifier
                .fillMaxWidth()
                .height(56.dp),
            shape = RoundedCornerShape(16.dp),
        ) {
            if (isLoading) {
                CircularProgressIndicator(
                    modifier = Modifier.size(24.dp),
                    color = MaterialTheme.colorScheme.onPrimary,
                )
            } else {
                Text("Send ${amount.ifBlank { "0" }} ${state.balance.currency}")
            }
        }

        // Error
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
