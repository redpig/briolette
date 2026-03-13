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
 * Setup screen: merchant taps their credstick once to register.
 *
 * The PoS reads the merchant's SignedTicket via READ_TICKET (0x11)
 * and stores it locally. This ticket is used for all subsequent
 * transactions — the merchant credstick isn't needed again until
 * sweep time.
 */
@Composable
fun SetupScreen(
    state: PosState,
    onSetupComplete: () -> Unit,
) {
    Column(
        modifier = Modifier
            .fillMaxSize()
            .padding(24.dp),
        horizontalAlignment = Alignment.CenterHorizontally,
        verticalArrangement = Arrangement.Center,
    ) {
        Text(
            text = "Briolette PoS",
            style = MaterialTheme.typography.headlineMedium,
        )

        Spacer(modifier = Modifier.height(32.dp))

        if (!state.merchantConfigured) {
            // Step 1: Tap merchant credstick to register.
            Card(
                modifier = Modifier.fillMaxWidth(),
            ) {
                Column(
                    modifier = Modifier.padding(24.dp),
                    horizontalAlignment = Alignment.CenterHorizontally,
                ) {
                    Text(
                        text = "Tap merchant credstick",
                        style = MaterialTheme.typography.titleLarge,
                        textAlign = TextAlign.Center,
                    )

                    Spacer(modifier = Modifier.height(16.dp))

                    Text(
                        text = "Hold your merchant credstick against the phone to register it as the payment recipient.",
                        style = MaterialTheme.typography.bodyMedium,
                        textAlign = TextAlign.Center,
                    )

                    Spacer(modifier = Modifier.height(24.dp))

                    // NFC icon placeholder.
                    Text(
                        text = "NFC",
                        style = MaterialTheme.typography.displaySmall,
                    )
                }
            }
        } else {
            // Setup complete.
            Text(
                text = "Merchant registered",
                style = MaterialTheme.typography.titleLarge,
                color = MaterialTheme.colorScheme.primary,
            )

            Spacer(modifier = Modifier.height(8.dp))

            Text(
                text = "Ticket: ${state.merchantTicketB64.take(16)}...",
                style = MaterialTheme.typography.bodySmall,
            )

            Spacer(modifier = Modifier.height(24.dp))

            Button(onClick = onSetupComplete) {
                Text("Start Accepting Payments")
            }
        }
    }
}
