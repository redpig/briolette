package com.briolette.pos.ui.screens

import androidx.compose.foundation.layout.*
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.unit.dp
import com.briolette.pos.data.PosState

/**
 * Sweep screen: merchant collects accumulated tokens.
 *
 * The merchant taps their credstick to transfer all accumulated
 * tokens from the PoS phone to the credstick. This is like
 * emptying the cash register into the safe.
 */
@Composable
fun SweepScreen(
    state: PosState,
    sweepInProgress: Boolean,
    sweepSuccess: Boolean?,
    onStartSweep: () -> Unit,
    onDone: () -> Unit,
) {
    Column(
        modifier = Modifier
            .fillMaxSize()
            .padding(24.dp),
        horizontalAlignment = Alignment.CenterHorizontally,
        verticalArrangement = Arrangement.Center,
    ) {
        Text(
            text = "Collect Tokens",
            style = MaterialTheme.typography.headlineMedium,
        )

        Spacer(modifier = Modifier.height(24.dp))

        Card(modifier = Modifier.fillMaxWidth()) {
            Column(
                modifier = Modifier.padding(24.dp),
                horizontalAlignment = Alignment.CenterHorizontally,
            ) {
                Text(
                    text = "Accumulated:",
                    style = MaterialTheme.typography.titleMedium,
                )

                Spacer(modifier = Modifier.height(8.dp))

                Text(
                    text = "${state.totalAccumulated} tokens",
                    style = MaterialTheme.typography.headlineLarge,
                )

                Spacer(modifier = Modifier.height(4.dp))

                Text(
                    text = "${state.validatedCount} validated, ${state.unvalidatedCount} pending",
                    style = MaterialTheme.typography.bodySmall,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
            }
        }

        Spacer(modifier = Modifier.height(24.dp))

        when {
            sweepSuccess == true -> {
                Text(
                    text = "Tokens collected!",
                    style = MaterialTheme.typography.titleLarge,
                    color = MaterialTheme.colorScheme.primary,
                    textAlign = TextAlign.Center,
                )
                Spacer(modifier = Modifier.height(16.dp))
                Button(onClick = onDone) {
                    Text("Done")
                }
            }

            sweepSuccess == false -> {
                Text(
                    text = "Sweep failed. Try again.",
                    style = MaterialTheme.typography.titleMedium,
                    color = MaterialTheme.colorScheme.error,
                )
                Spacer(modifier = Modifier.height(16.dp))
                Button(onClick = onStartSweep) {
                    Text("Retry")
                }
            }

            sweepInProgress -> {
                Text(
                    text = "Tap merchant credstick to collect",
                    style = MaterialTheme.typography.titleLarge,
                    textAlign = TextAlign.Center,
                )
                Spacer(modifier = Modifier.height(16.dp))
                CircularProgressIndicator()
            }

            else -> {
                if (state.totalAccumulated > 0) {
                    Button(
                        onClick = onStartSweep,
                        modifier = Modifier.fillMaxWidth(),
                    ) {
                        Text("Tap Merchant Credstick to Collect")
                    }
                } else {
                    Text(
                        text = "No tokens to collect",
                        style = MaterialTheme.typography.bodyLarge,
                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                    )
                }
            }
        }
    }
}
