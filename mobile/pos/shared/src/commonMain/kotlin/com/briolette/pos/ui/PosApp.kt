package com.briolette.pos.ui

import androidx.compose.foundation.layout.padding
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.Modifier
import com.briolette.pos.data.PosRepository
import com.briolette.pos.data.TransactionPhase
import com.briolette.pos.ui.screens.PaymentScreen
import com.briolette.pos.ui.screens.SetupScreen
import com.briolette.pos.ui.screens.SweepScreen

/**
 * PoS app root composable.
 *
 * Navigation:
 * - Setup: shown when merchant ticket not configured
 * - Payment: main screen for accepting payments
 * - Sweep: collect accumulated tokens
 */
@Composable
fun PosApp(repository: PosRepository) {
    val state by repository.state.collectAsState()
    val phase by repository.phase.collectAsState()
    val recentPayments by repository.recentPayments.collectAsState()

    var currentScreen by remember { mutableStateOf(PosScreen.Payment) }
    var sweepInProgress by remember { mutableStateOf(false) }
    var sweepSuccess by remember { mutableStateOf<Boolean?>(null) }

    // Force setup if merchant not configured.
    if (!state.merchantConfigured) {
        SetupScreen(
            state = state,
            onSetupComplete = { currentScreen = PosScreen.Payment },
        )
        return
    }

    Scaffold(
        bottomBar = {
            NavigationBar {
                NavigationBarItem(
                    selected = currentScreen == PosScreen.Payment,
                    onClick = { currentScreen = PosScreen.Payment },
                    label = { Text("Pay") },
                    icon = {},
                )
                NavigationBarItem(
                    selected = currentScreen == PosScreen.Sweep,
                    onClick = {
                        currentScreen = PosScreen.Sweep
                        sweepSuccess = null
                        sweepInProgress = false
                    },
                    label = { Text("Collect") },
                    icon = {},
                )
            }
        }
    ) { padding ->
        when (currentScreen) {
            PosScreen.Payment -> {
                PaymentScreen(
                    phase = phase,
                    recentPayments = recentPayments,
                    onStartPayment = { amount, desc ->
                        repository.startPayment(amount, desc)
                    },
                    onNewPayment = { repository.resetTransaction() },
                    modifier = Modifier.padding(padding),
                )
            }
            PosScreen.Sweep -> {
                SweepScreen(
                    state = state,
                    sweepInProgress = sweepInProgress,
                    sweepSuccess = sweepSuccess,
                    onStartSweep = { sweepInProgress = true },
                    onDone = {
                        currentScreen = PosScreen.Payment
                        sweepSuccess = null
                    },
                )
            }
        }
    }
}

private enum class PosScreen {
    Payment,
    Sweep,
}

// Add modifier parameter to PaymentScreen for scaffold padding.
@Composable
private fun PaymentScreen(
    phase: TransactionPhase,
    recentPayments: List<com.briolette.pos.data.PaymentRecord>,
    onStartPayment: (Int, String) -> Unit,
    onNewPayment: () -> Unit,
    modifier: Modifier = Modifier,
) {
    // Delegate to the actual screen with the modifier applied.
    // The actual PaymentScreen handles its own layout.
    androidx.compose.foundation.layout.Box(modifier = modifier) {
        com.briolette.pos.ui.screens.PaymentScreen(
            phase = phase,
            recentPayments = recentPayments,
            onStartPayment = onStartPayment,
            onNewPayment = onNewPayment,
        )
    }
}
