package com.briolette.wallet.ui.screens

import androidx.compose.foundation.layout.*
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import com.briolette.wallet.data.NetworkConfig
import com.briolette.wallet.data.NfcCardProvider
import com.briolette.wallet.data.SecurityMode
import com.briolette.wallet.data.WalletRepository
import kotlinx.coroutines.launch

/**
 * Initial wallet setup screen.
 *
 * Shown when no saved wallet exists. Creates a new wallet, registers
 * with the network, and fetches initial tickets.
 */
@Composable
fun SetupScreen(
    repository: WalletRepository,
    onSetupComplete: () -> Unit,
    nfcCardProvider: NfcCardProvider? = null,
) {
    val isLoading by repository.isLoading.collectAsState()
    val error by repository.error.collectAsState()
    val scope = rememberCoroutineScope()

    var walletName by remember { mutableStateOf("") }
    var serverHost by remember { mutableStateOf("127.0.0.1") }
    var showAdvanced by remember { mutableStateOf(false) }
    var securityMode by remember { mutableStateOf(SecurityMode.MEDIUM) }
    val nfcAvailable = nfcCardProvider?.isAvailable == true

    Column(
        modifier = Modifier
            .fillMaxSize()
            .padding(32.dp),
        horizontalAlignment = Alignment.CenterHorizontally,
        verticalArrangement = Arrangement.Center,
    ) {
        Text(
            text = "Briolette",
            fontSize = 36.sp,
            fontWeight = FontWeight.Bold,
            color = MaterialTheme.colorScheme.primary,
        )
        Text(
            text = "Private Digital Currency",
            style = MaterialTheme.typography.bodyLarge,
            color = MaterialTheme.colorScheme.onBackground.copy(alpha = 0.6f),
        )

        Spacer(Modifier.height(48.dp))

        OutlinedTextField(
            value = walletName,
            onValueChange = { walletName = it },
            label = { Text("Wallet Name") },
            singleLine = true,
            modifier = Modifier.fillMaxWidth(),
            shape = RoundedCornerShape(12.dp),
        )

        Spacer(Modifier.height(16.dp))

        // Security mode selector
        Text(
            text = "Security Mode",
            style = MaterialTheme.typography.labelLarge,
            modifier = Modifier.fillMaxWidth(),
        )
        Spacer(Modifier.height(4.dp))
        Row(
            modifier = Modifier.fillMaxWidth(),
            horizontalArrangement = Arrangement.spacedBy(8.dp),
        ) {
            FilterChip(
                selected = securityMode == SecurityMode.MEDIUM,
                onClick = { securityMode = SecurityMode.MEDIUM },
                label = { Text("Medium") },
                modifier = Modifier.weight(1f),
            )
            FilterChip(
                selected = securityMode == SecurityMode.HIGH,
                onClick = { securityMode = SecurityMode.HIGH },
                label = { Text("High") },
                modifier = Modifier.weight(1f),
            )
        }
        Text(
            text = if (securityMode == SecurityMode.HIGH)
                "Phone attestation + NFC smartcard split-key"
            else
                "Phone attestation only",
            style = MaterialTheme.typography.bodySmall,
            color = MaterialTheme.colorScheme.onBackground.copy(alpha = 0.5f),
        )
        if (securityMode == SecurityMode.HIGH && !nfcAvailable) {
            Text(
                text = "NFC smartcard will be required during setup",
                style = MaterialTheme.typography.bodySmall,
                color = MaterialTheme.colorScheme.error.copy(alpha = 0.8f),
            )
        }

        Spacer(Modifier.height(12.dp))

        TextButton(onClick = { showAdvanced = !showAdvanced }) {
            Text(if (showAdvanced) "Hide server settings" else "Server settings")
        }

        if (showAdvanced) {
            OutlinedTextField(
                value = serverHost,
                onValueChange = { serverHost = it },
                label = { Text("Server Host") },
                singleLine = true,
                modifier = Modifier.fillMaxWidth(),
                shape = RoundedCornerShape(12.dp),
            )
            Spacer(Modifier.height(8.dp))
            Text(
                text = "Registrar :50051, Clerk :50052, Mint :50053, Validate :50055",
                style = MaterialTheme.typography.bodySmall,
                color = MaterialTheme.colorScheme.onBackground.copy(alpha = 0.5f),
            )
        }

        Spacer(Modifier.height(32.dp))

        Button(
            onClick = {
                scope.launch {
                    try {
                        val config = NetworkConfig(
                            registrarUri = "http://$serverHost:50051",
                            clerkUri = "http://$serverHost:50052",
                            mintUri = "http://$serverHost:50053",
                            validateUri = "http://$serverHost:50055",
                        )
                        val name = walletName.ifBlank { "wallet" }
                        repository.createWallet(
                            name,
                            config,
                            securityMode = securityMode,
                            nfcCardProvider = if (securityMode == SecurityMode.HIGH) nfcCardProvider else null,
                        )
                        onSetupComplete()
                    } catch (_: Exception) {
                        // Error shown via repository.error
                    }
                }
            },
            enabled = !isLoading,
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
                Text("Create Wallet")
            }
        }

        error?.let { msg ->
            Spacer(Modifier.height(16.dp))
            Card(
                colors = CardDefaults.cardColors(
                    containerColor = MaterialTheme.colorScheme.errorContainer,
                ),
                modifier = Modifier.fillMaxWidth(),
            ) {
                Text(
                    text = msg,
                    modifier = Modifier.padding(16.dp),
                    color = MaterialTheme.colorScheme.onErrorContainer,
                )
            }
        }
    }
}
