package com.briolette.wallet.ui.screens

import androidx.compose.foundation.layout.*
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import com.briolette.wallet.data.NetworkConfig
import com.briolette.wallet.data.WalletRepository
import kotlinx.coroutines.launch

/**
 * Settings screen for server configuration, wallet management,
 * and ticket lifecycle operations.
 */
@Composable
fun SettingsScreen(
    repository: WalletRepository,
    currentConfig: NetworkConfig,
    onConfigChanged: (NetworkConfig) -> Unit,
    onBack: () -> Unit,
    onResetWallet: () -> Unit,
) {
    val state by repository.state.collectAsState()
    val isLoading by repository.isLoading.collectAsState()
    val error by repository.error.collectAsState()
    val scope = rememberCoroutineScope()

    var serverHost by remember {
        mutableStateOf(
            currentConfig.registrarUri
                .removePrefix("http://")
                .removeSuffix(":50051")
        )
    }
    var showResetConfirm by remember { mutableStateOf(false) }
    var refreshResult by remember { mutableStateOf<String?>(null) }

    Column(
        modifier = Modifier
            .fillMaxSize()
            .verticalScroll(rememberScrollState())
            .padding(16.dp),
    ) {
        // Top bar
        Row(
            modifier = Modifier.fillMaxWidth(),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            TextButton(onClick = onBack) { Text("Back") }
            Spacer(Modifier.weight(1f))
            Text("Settings", style = MaterialTheme.typography.titleMedium)
            Spacer(Modifier.weight(1f))
            Spacer(Modifier.width(48.dp))
        }

        Spacer(Modifier.height(24.dp))

        // ── Wallet Info ──
        SectionHeader("Wallet")
        Card(modifier = Modifier.fillMaxWidth()) {
            Column(modifier = Modifier.padding(16.dp)) {
                InfoRow("Name", state.walletName.ifBlank { "Not set" })
                InfoRow("Tokens", "${state.balance.tokenCount}")
                InfoRow("Tickets", "${state.ticketCount}")
                InfoRow("Balance", "${state.balance.displayAmount} ${state.balance.currency}")
            }
        }

        Spacer(Modifier.height(24.dp))

        // ── Network Configuration ──
        SectionHeader("Network")
        Card(modifier = Modifier.fillMaxWidth()) {
            Column(modifier = Modifier.padding(16.dp)) {
                OutlinedTextField(
                    value = serverHost,
                    onValueChange = { serverHost = it },
                    label = { Text("Server Host") },
                    singleLine = true,
                    modifier = Modifier.fillMaxWidth(),
                    shape = RoundedCornerShape(8.dp),
                )
                Spacer(Modifier.height(8.dp))
                Text(
                    "Services: Registrar :50051, Clerk :50052, Mint :50053, Validate :50055",
                    style = MaterialTheme.typography.bodySmall,
                    color = MaterialTheme.colorScheme.onSurface.copy(alpha = 0.5f),
                )
                Spacer(Modifier.height(12.dp))
                Button(
                    onClick = {
                        val config = NetworkConfig(
                            registrarUri = "http://$serverHost:50051",
                            clerkUri = "http://$serverHost:50052",
                            mintUri = "http://$serverHost:50053",
                            validateUri = "http://$serverHost:50055",
                        )
                        onConfigChanged(config)
                    },
                    modifier = Modifier.fillMaxWidth(),
                ) {
                    Text("Save Server Config")
                }
            }
        }

        Spacer(Modifier.height(24.dp))

        // ── Ticket Management ──
        SectionHeader("Ticket Management")
        Card(modifier = Modifier.fillMaxWidth()) {
            Column(modifier = Modifier.padding(16.dp)) {
                Text(
                    "Tickets are time-bound receiving addresses (~24h validity). " +
                        "Request more when running low, or refresh expiring ones.",
                    style = MaterialTheme.typography.bodySmall,
                    color = MaterialTheme.colorScheme.onSurface.copy(alpha = 0.6f),
                )
                Spacer(Modifier.height(12.dp))
                Row(
                    modifier = Modifier.fillMaxWidth(),
                    horizontalArrangement = Arrangement.spacedBy(8.dp),
                ) {
                    OutlinedButton(
                        onClick = {
                            scope.launch {
                                try {
                                    repository.requestTickets(10)
                                    refreshResult = "Requested 10 tickets"
                                } catch (e: Exception) {
                                    refreshResult = "Failed: ${e.message}"
                                }
                            }
                        },
                        enabled = !isLoading,
                        modifier = Modifier.weight(1f),
                    ) {
                        Text("Request 10")
                    }
                    OutlinedButton(
                        onClick = {
                            scope.launch {
                                try {
                                    repository.synchronize()
                                    refreshResult = "Epoch synced"
                                } catch (e: Exception) {
                                    refreshResult = "Sync failed: ${e.message}"
                                }
                            }
                        },
                        enabled = !isLoading,
                        modifier = Modifier.weight(1f),
                    ) {
                        Text("Sync Epoch")
                    }
                }
                refreshResult?.let { msg ->
                    Spacer(Modifier.height(8.dp))
                    Text(msg, style = MaterialTheme.typography.bodySmall)
                }
            }
        }

        Spacer(Modifier.height(24.dp))

        // ── Danger Zone ──
        SectionHeader("Danger Zone")
        Card(
            modifier = Modifier.fillMaxWidth(),
            colors = CardDefaults.cardColors(
                containerColor = MaterialTheme.colorScheme.errorContainer.copy(alpha = 0.3f),
            ),
        ) {
            Column(modifier = Modifier.padding(16.dp)) {
                Text(
                    "Resetting the wallet will delete all local data including " +
                        "tokens and credentials. This cannot be undone.",
                    style = MaterialTheme.typography.bodySmall,
                    color = MaterialTheme.colorScheme.error,
                )
                Spacer(Modifier.height(12.dp))
                if (showResetConfirm) {
                    Row(
                        modifier = Modifier.fillMaxWidth(),
                        horizontalArrangement = Arrangement.spacedBy(8.dp),
                    ) {
                        OutlinedButton(
                            onClick = { showResetConfirm = false },
                            modifier = Modifier.weight(1f),
                        ) {
                            Text("Cancel")
                        }
                        Button(
                            onClick = onResetWallet,
                            modifier = Modifier.weight(1f),
                            colors = ButtonDefaults.buttonColors(
                                containerColor = MaterialTheme.colorScheme.error,
                            ),
                        ) {
                            Text("Confirm Reset")
                        }
                    }
                } else {
                    OutlinedButton(
                        onClick = { showResetConfirm = true },
                        modifier = Modifier.fillMaxWidth(),
                        colors = ButtonDefaults.outlinedButtonColors(
                            contentColor = MaterialTheme.colorScheme.error,
                        ),
                    ) {
                        Text("Reset Wallet")
                    }
                }
            }
        }

        Spacer(Modifier.height(16.dp))

        // Loading indicator
        if (isLoading) {
            LinearProgressIndicator(modifier = Modifier.fillMaxWidth())
        }

        // Error
        error?.let { msg ->
            Spacer(Modifier.height(8.dp))
            Text(msg, color = MaterialTheme.colorScheme.error)
        }

        Spacer(Modifier.height(32.dp))
    }
}

@Composable
private fun SectionHeader(title: String) {
    Text(
        text = title,
        style = MaterialTheme.typography.titleSmall,
        fontWeight = FontWeight.Bold,
        color = MaterialTheme.colorScheme.primary,
        modifier = Modifier.padding(bottom = 8.dp),
    )
}

@Composable
private fun InfoRow(label: String, value: String) {
    Row(
        modifier = Modifier.fillMaxWidth().padding(vertical = 4.dp),
    ) {
        Text(
            label,
            style = MaterialTheme.typography.bodyMedium,
            color = MaterialTheme.colorScheme.onSurface.copy(alpha = 0.6f),
            modifier = Modifier.weight(1f),
        )
        Text(
            value,
            style = MaterialTheme.typography.bodyMedium,
            fontWeight = FontWeight.Medium,
        )
    }
}
