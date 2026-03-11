package com.briolette.wallet.ui.screens

import androidx.compose.foundation.layout.*
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import com.briolette.wallet.data.WalletRepository
import kotlinx.coroutines.launch

/**
 * Main balance screen — the home screen of the wallet app.
 *
 * Shows the current balance, ticket count, and action buttons for
 * Pay, Receive, Top Up, and more.
 */
@Composable
fun BalanceScreen(
    repository: WalletRepository,
    onNavigatePay: () -> Unit,
    onNavigateReceive: () -> Unit,
    onNavigateTopUp: () -> Unit,
    onNavigateMyQr: () -> Unit,
    onNavigateHistory: () -> Unit = {},
) {
    val state by repository.state.collectAsState()
    val isLoading by repository.isLoading.collectAsState()
    val error by repository.error.collectAsState()
    val scope = rememberCoroutineScope()

    Column(
        modifier = Modifier
            .fillMaxSize()
            .padding(24.dp),
        horizontalAlignment = Alignment.CenterHorizontally,
    ) {
        Spacer(Modifier.height(32.dp))

        // Wallet name
        Text(
            text = state.walletName.ifBlank { "Briolette Wallet" },
            style = MaterialTheme.typography.titleMedium,
            color = MaterialTheme.colorScheme.onBackground.copy(alpha = 0.7f),
        )

        Spacer(Modifier.height(16.dp))

        // Balance card
        Card(
            modifier = Modifier.fillMaxWidth(),
            shape = RoundedCornerShape(20.dp),
            colors = CardDefaults.cardColors(
                containerColor = MaterialTheme.colorScheme.primary,
            ),
        ) {
            Column(
                modifier = Modifier
                    .fillMaxWidth()
                    .padding(32.dp),
                horizontalAlignment = Alignment.CenterHorizontally,
            ) {
                Text(
                    text = "Balance",
                    style = MaterialTheme.typography.labelLarge,
                    color = MaterialTheme.colorScheme.onPrimary.copy(alpha = 0.8f),
                )
                Spacer(Modifier.height(8.dp))
                Text(
                    text = state.balance.displayAmount,
                    fontSize = 48.sp,
                    fontWeight = FontWeight.Bold,
                    color = MaterialTheme.colorScheme.onPrimary,
                )
                Text(
                    text = state.balance.currency,
                    style = MaterialTheme.typography.titleMedium,
                    color = MaterialTheme.colorScheme.onPrimary.copy(alpha = 0.7f),
                )
                Spacer(Modifier.height(16.dp))
                Row(
                    modifier = Modifier.fillMaxWidth(),
                    horizontalArrangement = Arrangement.SpaceEvenly,
                ) {
                    Column(horizontalAlignment = Alignment.CenterHorizontally) {
                        Text(
                            text = "${state.balance.tokenCount}",
                            fontWeight = FontWeight.Bold,
                            color = MaterialTheme.colorScheme.onPrimary,
                        )
                        Text(
                            text = "Tokens",
                            style = MaterialTheme.typography.bodySmall,
                            color = MaterialTheme.colorScheme.onPrimary.copy(alpha = 0.7f),
                        )
                    }
                    Column(horizontalAlignment = Alignment.CenterHorizontally) {
                        Text(
                            text = "${state.ticketCount}",
                            fontWeight = FontWeight.Bold,
                            color = MaterialTheme.colorScheme.onPrimary,
                        )
                        Text(
                            text = "Tickets",
                            style = MaterialTheme.typography.bodySmall,
                            color = MaterialTheme.colorScheme.onPrimary.copy(alpha = 0.7f),
                        )
                    }
                }
            }
        }

        Spacer(Modifier.height(32.dp))

        // Action buttons grid
        Row(
            modifier = Modifier.fillMaxWidth(),
            horizontalArrangement = Arrangement.spacedBy(12.dp),
        ) {
            ActionButton(
                label = "Pay",
                enabled = state.canPay && !isLoading,
                modifier = Modifier.weight(1f),
                onClick = onNavigatePay,
            )
            ActionButton(
                label = "Receive",
                enabled = state.canReceive && !isLoading,
                modifier = Modifier.weight(1f),
                onClick = onNavigateReceive,
            )
        }

        Spacer(Modifier.height(12.dp))

        Row(
            modifier = Modifier.fillMaxWidth(),
            horizontalArrangement = Arrangement.spacedBy(12.dp),
        ) {
            ActionButton(
                label = "Top Up",
                enabled = state.canReceive && !isLoading,
                modifier = Modifier.weight(1f),
                onClick = onNavigateTopUp,
            )
            ActionButton(
                label = "My QR",
                enabled = state.canReceive && !isLoading,
                modifier = Modifier.weight(1f),
                onClick = onNavigateMyQr,
            )
        }

        Spacer(Modifier.height(24.dp))

        // Sync & validate row
        Row(
            modifier = Modifier.fillMaxWidth(),
            horizontalArrangement = Arrangement.spacedBy(12.dp),
        ) {
            OutlinedButton(
                onClick = { scope.launch { repository.synchronize() } },
                enabled = !isLoading,
                modifier = Modifier.weight(1f),
            ) {
                Text("Sync")
            }
            OutlinedButton(
                onClick = { scope.launch { repository.validate() } },
                enabled = state.balance.tokenCount > 0 && !isLoading,
                modifier = Modifier.weight(1f),
            ) {
                Text("Validate")
            }
            OutlinedButton(
                onClick = { scope.launch { repository.requestTickets(5) } },
                enabled = !isLoading,
                modifier = Modifier.weight(1f),
            ) {
                Text("+ Tickets")
            }
        }

        Spacer(Modifier.height(12.dp))

        // Token inventory
        OutlinedButton(
            onClick = onNavigateHistory,
            enabled = !isLoading,
            modifier = Modifier.fillMaxWidth(),
        ) {
            Text("Token Inventory")
        }

        // Loading indicator
        if (isLoading) {
            Spacer(Modifier.height(16.dp))
            CircularProgressIndicator()
        }

        // Error display
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
                    textAlign = TextAlign.Center,
                )
            }
        }
    }
}

@Composable
private fun ActionButton(
    label: String,
    enabled: Boolean,
    modifier: Modifier = Modifier,
    onClick: () -> Unit,
) {
    Button(
        onClick = onClick,
        enabled = enabled,
        modifier = modifier.height(56.dp),
        shape = RoundedCornerShape(16.dp),
    ) {
        Text(label, fontSize = 16.sp)
    }
}
