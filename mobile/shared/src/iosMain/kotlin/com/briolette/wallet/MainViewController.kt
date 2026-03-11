package com.briolette.wallet

import androidx.compose.foundation.layout.*
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.unit.dp
import androidx.compose.ui.window.ComposeUIViewController
import com.briolette.wallet.data.WalletRepository
import com.briolette.wallet.navigation.NavRoutes
import com.briolette.wallet.ui.screens.*
import com.briolette.wallet.ui.theme.BrioletteTheme
import platform.UIKit.UIViewController

/**
 * Entry point for the Compose Multiplatform UI on iOS.
 *
 * Called from Swift via:
 *   let vc = MainViewControllerKt.MainViewController()
 *
 * Provides the same navigation and screens as the Android app,
 * using a simple state-based navigation (no AndroidX NavHost on iOS).
 */
fun MainViewController(): UIViewController {
    val persistence = IosWalletPersistence()
    val bridge = IosWalletBridge()
    val repository = WalletRepository(bridge, persistence)
    val qrGenerator = IosQrCodeGenerator()

    return ComposeUIViewController {
        BrioletteTheme {
            IosWalletApp(repository, qrGenerator)
        }
    }
}

/**
 * iOS wallet app with simple stack-based navigation.
 *
 * Since AndroidX Navigation isn't available on iOS, we use a
 * mutableStateOf-based navigation stack. Same screens, different nav.
 */
@Composable
private fun IosWalletApp(
    repository: WalletRepository,
    qrGenerator: IosQrCodeGenerator,
) {
    var currentRoute by remember { mutableStateOf(NavRoutes.SETUP) }
    val state by repository.state.collectAsState()
    val routeStack = remember { mutableStateListOf<String>() }

    // Try loading saved wallet
    LaunchedEffect(Unit) {
        if (repository.tryLoadSaved()) {
            currentRoute = NavRoutes.BALANCE
        }
    }

    // Simple navigation helpers
    fun navigate(route: String) {
        routeStack.add(currentRoute)
        currentRoute = route
    }
    fun popBack() {
        currentRoute = routeStack.removeLastOrNull() ?: NavRoutes.BALANCE
    }
    fun popToBalance() {
        routeStack.clear()
        currentRoute = NavRoutes.BALANCE
    }

    // Shared state
    var scannedPayTicket by remember { mutableStateOf<String?>(null) }
    var scannedReceiveTokens by remember { mutableStateOf<List<String>?>(null) }

    Surface(modifier = Modifier.fillMaxSize()) {
        when (currentRoute) {
            NavRoutes.SETUP -> SetupScreen(
                repository = repository,
                onSetupComplete = {
                    routeStack.clear()
                    currentRoute = NavRoutes.BALANCE
                },
            )

            NavRoutes.BALANCE -> BalanceScreen(
                repository = repository,
                onNavigatePay = {
                    scannedPayTicket = null
                    navigate(NavRoutes.PAY)
                },
                onNavigateReceive = {
                    scannedReceiveTokens = null
                    navigate(NavRoutes.RECEIVE)
                },
                onNavigateTopUp = { navigate(NavRoutes.TOP_UP) },
                onNavigateMyQr = { navigate(NavRoutes.MY_QR) },
                onNavigateHistory = { navigate(NavRoutes.HISTORY) },
            )

            NavRoutes.HISTORY -> HistoryScreen(
                repository = repository,
                onBack = { popBack() },
            )

            NavRoutes.MY_QR -> MyQrScreen(
                repository = repository,
                qrGenerator = qrGenerator,
                onBack = { popBack() },
            )

            NavRoutes.PAY -> PayScreen(
                repository = repository,
                scannedTicketB64 = scannedPayTicket,
                onScanQr = {
                    // On iOS, present a native scanner view controller
                    // For now, this is a stub
                    navigate(NavRoutes.PAY_SCAN)
                },
                onBack = { popBack() },
                onShowTransferQr = { popToBalance() },
            )

            NavRoutes.RECEIVE -> ReceiveScreen(
                repository = repository,
                scannedTokensB64 = scannedReceiveTokens,
                onScanQr = { navigate(NavRoutes.RECEIVE_SCAN) },
                onBack = { popBack() },
            )

            NavRoutes.TOP_UP -> TopUpScreen(
                repository = repository,
                onBack = { popBack() },
            )

            // Scanner placeholders for iOS (replace with AVFoundation)
            NavRoutes.PAY_SCAN, NavRoutes.RECEIVE_SCAN -> {
                IosScannerPlaceholder(
                    title = if (currentRoute == NavRoutes.PAY_SCAN) "Scan Recipient"
                    else "Scan Payment",
                    onResult = { result ->
                        if (currentRoute == NavRoutes.PAY_SCAN) {
                            scannedPayTicket = result
                        } else {
                            scannedReceiveTokens = listOf(result)
                        }
                        popBack()
                    },
                    onBack = { popBack() },
                )
            }
        }
    }
}

/**
 * Placeholder scanner for iOS.
 *
 * Replace with AVCaptureSession + CIDetector barcode scanning
 * wrapped in a UIViewControllerRepresentable.
 */
@Composable
private fun IosScannerPlaceholder(
    title: String,
    onResult: (String) -> Unit,
    onBack: () -> Unit,
) {
    var input by remember { mutableStateOf("") }

    Column(
        modifier = Modifier.fillMaxSize().padding(24.dp),
        horizontalAlignment = Alignment.CenterHorizontally,
        verticalArrangement = Arrangement.Center,
    ) {
        Text(title, style = MaterialTheme.typography.titleLarge)
        Spacer(Modifier.height(16.dp))
        Text(
            "iOS camera scanner coming soon.\nPaste data below for testing:",
            style = MaterialTheme.typography.bodySmall,
        )
        Spacer(Modifier.height(8.dp))
        OutlinedTextField(
            value = input,
            onValueChange = { input = it },
            label = { Text("Base64 data") },
            modifier = Modifier.fillMaxWidth(),
        )
        Spacer(Modifier.height(16.dp))
        Row(modifier = Modifier.fillMaxWidth()) {
            TextButton(onClick = onBack) { Text("Cancel") }
            Spacer(Modifier.weight(1f))
            Button(onClick = { if (input.isNotBlank()) onResult(input) }) {
                Text("Use Data")
            }
        }
    }
}
