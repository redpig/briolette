package com.briolette.pos.ui.screens

import androidx.compose.foundation.layout.*
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.text.input.KeyboardType
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.unit.dp
import com.briolette.pos.data.PaymentRecord
import com.briolette.pos.data.TransactionPhase
import com.briolette.pos.data.ValidationStatus

/**
 * Main payment screen: amount entry + tap-to-pay + recent transactions.
 *
 * Flow:
 * 1. Merchant enters amount and optional description
 * 2. Presses "Start Payment" → screen switches to NFC waiting mode
 * 3. Customer taps credstick (tap 1) → proposal sent, validation begins
 * 4. Screen shows validation status, prompts "tap again to confirm"
 * 5. Customer taps again (tap 2) → signed tokens received
 * 6. Screen shows result
 */
@Composable
fun PaymentScreen(
    phase: TransactionPhase,
    recentPayments: List<PaymentRecord>,
    onStartPayment: (amount: Int, description: String) -> Unit,
    onNewPayment: () -> Unit,
) {
    Column(
        modifier = Modifier
            .fillMaxSize()
            .padding(16.dp),
    ) {
        // Header.
        Text(
            text = "Briolette PoS",
            style = MaterialTheme.typography.titleMedium,
        )

        Spacer(modifier = Modifier.height(16.dp))

        when (phase) {
            is TransactionPhase.Idle -> {
                AmountEntrySection(onStartPayment = onStartPayment)
            }

            is TransactionPhase.WaitingForTap -> {
                WaitingForTapSection(amount = phase.amount)
            }

            is TransactionPhase.ProposalSent -> {
                ProposalSentSection(phase = phase)
            }

            is TransactionPhase.Complete -> {
                CompleteSection(
                    amount = phase.amount,
                    validated = phase.validated,
                    onNewPayment = onNewPayment,
                )
            }

            is TransactionPhase.Failed -> {
                FailedSection(
                    reason = phase.reason,
                    onRetry = onNewPayment,
                )
            }
        }

        Spacer(modifier = Modifier.height(24.dp))

        // Recent payments.
        if (recentPayments.isNotEmpty()) {
            HorizontalDivider()
            Spacer(modifier = Modifier.height(8.dp))
            Text(
                text = "Recent",
                style = MaterialTheme.typography.titleSmall,
            )
            Spacer(modifier = Modifier.height(4.dp))
            for (payment in recentPayments.take(5)) {
                PaymentRow(payment)
            }
        }
    }
}

@Composable
private fun AmountEntrySection(
    onStartPayment: (Int, String) -> Unit,
) {
    var amount by remember { mutableStateOf("") }
    var description by remember { mutableStateOf("") }

    Card(modifier = Modifier.fillMaxWidth()) {
        Column(modifier = Modifier.padding(16.dp)) {
            OutlinedTextField(
                value = amount,
                onValueChange = { amount = it.filter { c -> c.isDigit() } },
                label = { Text("Amount (tokens)") },
                keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Number),
                modifier = Modifier.fillMaxWidth(),
                singleLine = true,
            )

            Spacer(modifier = Modifier.height(8.dp))

            OutlinedTextField(
                value = description,
                onValueChange = { description = it.take(32) },
                label = { Text("Description (optional)") },
                modifier = Modifier.fillMaxWidth(),
                singleLine = true,
            )

            Spacer(modifier = Modifier.height(16.dp))

            Button(
                onClick = {
                    val amt = amount.toIntOrNull() ?: 0
                    if (amt > 0) {
                        onStartPayment(amt, description)
                    }
                },
                modifier = Modifier.fillMaxWidth(),
                enabled = amount.toIntOrNull()?.let { it > 0 } == true,
            ) {
                Text("Start Payment")
            }
        }
    }
}

@Composable
private fun WaitingForTapSection(amount: Int) {
    Card(
        modifier = Modifier.fillMaxWidth(),
    ) {
        Column(
            modifier = Modifier
                .padding(32.dp)
                .fillMaxWidth(),
            horizontalAlignment = Alignment.CenterHorizontally,
        ) {
            Text(
                text = "Tap customer's credstick",
                style = MaterialTheme.typography.titleLarge,
                textAlign = TextAlign.Center,
            )

            Spacer(modifier = Modifier.height(8.dp))

            Text(
                text = "(tap 1 of 2)",
                style = MaterialTheme.typography.bodyMedium,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )

            Spacer(modifier = Modifier.height(24.dp))

            Text(
                text = "$amount tokens",
                style = MaterialTheme.typography.headlineLarge,
            )
        }
    }
}

@Composable
private fun ProposalSentSection(phase: TransactionPhase.ProposalSent) {
    Card(
        modifier = Modifier.fillMaxWidth(),
    ) {
        Column(
            modifier = Modifier
                .padding(24.dp)
                .fillMaxWidth(),
            horizontalAlignment = Alignment.CenterHorizontally,
        ) {
            Text(
                text = "Validating tokens...",
                style = MaterialTheme.typography.titleMedium,
            )

            Spacer(modifier = Modifier.height(12.dp))

            // Validation status indicators.
            Row(verticalAlignment = Alignment.CenterVertically) {
                Text(if (phase.cryptoValid) "OK" else "...", style = MaterialTheme.typography.bodySmall)
                Spacer(modifier = Modifier.width(8.dp))
                Text("Crypto valid", style = MaterialTheme.typography.bodyMedium)
            }

            Row(verticalAlignment = Alignment.CenterVertically) {
                val statusText = when (phase.onlineValid) {
                    true -> "OK"
                    false -> "FAIL"
                    null -> "..."
                }
                Text(statusText, style = MaterialTheme.typography.bodySmall)
                Spacer(modifier = Modifier.width(8.dp))
                Text("Tokenmap check", style = MaterialTheme.typography.bodyMedium)
            }

            Spacer(modifier = Modifier.height(24.dp))

            Text(
                text = "Tap again to confirm",
                style = MaterialTheme.typography.titleLarge,
                textAlign = TextAlign.Center,
            )

            Spacer(modifier = Modifier.height(8.dp))

            Text(
                text = "${phase.amount} tokens",
                style = MaterialTheme.typography.headlineMedium,
            )
        }
    }
}

@Composable
private fun CompleteSection(
    amount: Int,
    validated: Boolean,
    onNewPayment: () -> Unit,
) {
    Card(
        modifier = Modifier.fillMaxWidth(),
        colors = CardDefaults.cardColors(
            containerColor = MaterialTheme.colorScheme.primaryContainer,
        ),
    ) {
        Column(
            modifier = Modifier
                .padding(32.dp)
                .fillMaxWidth(),
            horizontalAlignment = Alignment.CenterHorizontally,
        ) {
            Text(
                text = "Received!",
                style = MaterialTheme.typography.headlineMedium,
                color = MaterialTheme.colorScheme.onPrimaryContainer,
            )

            Spacer(modifier = Modifier.height(8.dp))

            Text(
                text = "$amount tokens",
                style = MaterialTheme.typography.titleLarge,
                color = MaterialTheme.colorScheme.onPrimaryContainer,
            )

            Text(
                text = if (validated) "(validated)" else "(unvalidated)",
                style = MaterialTheme.typography.bodyMedium,
                color = MaterialTheme.colorScheme.onPrimaryContainer.copy(alpha = 0.7f),
            )

            Spacer(modifier = Modifier.height(24.dp))

            Button(onClick = onNewPayment) {
                Text("New Payment")
            }
        }
    }
}

@Composable
private fun FailedSection(reason: String, onRetry: () -> Unit) {
    Card(
        modifier = Modifier.fillMaxWidth(),
        colors = CardDefaults.cardColors(
            containerColor = MaterialTheme.colorScheme.errorContainer,
        ),
    ) {
        Column(
            modifier = Modifier
                .padding(24.dp)
                .fillMaxWidth(),
            horizontalAlignment = Alignment.CenterHorizontally,
        ) {
            Text(
                text = "Payment Failed",
                style = MaterialTheme.typography.titleLarge,
                color = MaterialTheme.colorScheme.onErrorContainer,
            )

            Spacer(modifier = Modifier.height(8.dp))

            Text(
                text = reason,
                style = MaterialTheme.typography.bodyMedium,
                color = MaterialTheme.colorScheme.onErrorContainer,
            )

            Spacer(modifier = Modifier.height(16.dp))

            Button(onClick = onRetry) {
                Text("Try Again")
            }
        }
    }
}

@Composable
private fun PaymentRow(payment: PaymentRecord) {
    Row(
        modifier = Modifier
            .fillMaxWidth()
            .padding(vertical = 4.dp),
        horizontalArrangement = Arrangement.SpaceBetween,
    ) {
        val statusIcon = when (payment.validationStatus) {
            ValidationStatus.VALID -> "OK"
            ValidationStatus.PENDING -> "..."
            ValidationStatus.INVALID -> "!!"
        }
        Text(
            text = "$statusIcon ${payment.amount} tokens",
            style = MaterialTheme.typography.bodyMedium,
        )
    }
}
