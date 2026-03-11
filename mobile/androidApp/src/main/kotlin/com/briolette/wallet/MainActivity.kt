package com.briolette.wallet

import android.os.Bundle
import androidx.activity.ComponentActivity
import androidx.activity.compose.setContent
import androidx.compose.foundation.layout.*
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.unit.dp
import androidx.navigation.compose.NavHost
import androidx.navigation.compose.composable
import androidx.navigation.compose.rememberNavController
import com.briolette.wallet.data.WalletRepository
import com.briolette.wallet.navigation.NavRoutes
import com.briolette.wallet.ui.screens.*
import com.briolette.wallet.ui.theme.BrioletteTheme

class MainActivity : ComponentActivity() {
    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)

        val persistence = AndroidWalletPersistence(applicationContext)
        val bridge = AndroidWalletBridge()
        val repository = WalletRepository(bridge, persistence)
        val qrGenerator = AndroidQrCodeGenerator()

        setContent {
            BrioletteTheme {
                WalletApp(repository, qrGenerator)
            }
        }
    }
}

@Composable
fun WalletApp(
    repository: WalletRepository,
    qrGenerator: AndroidQrCodeGenerator,
) {
    val navController = rememberNavController()
    val state by repository.state.collectAsState()

    // Scanned data shared between screens
    var scannedPayTicket by remember { mutableStateOf<String?>(null) }
    var scannedReceiveTokens by remember { mutableStateOf<List<String>?>(null) }

    // Try to load saved wallet on first composition
    LaunchedEffect(Unit) {
        val loaded = repository.tryLoadSaved()
        if (loaded) {
            navController.navigate(NavRoutes.BALANCE) {
                popUpTo(NavRoutes.SETUP) { inclusive = true }
            }
        }
    }

    val startDest = if (state.isInitialized) NavRoutes.BALANCE else NavRoutes.SETUP

    NavHost(navController = navController, startDestination = startDest) {
        composable(NavRoutes.SETUP) {
            SetupScreen(
                repository = repository,
                onSetupComplete = {
                    navController.navigate(NavRoutes.BALANCE) {
                        popUpTo(NavRoutes.SETUP) { inclusive = true }
                    }
                },
            )
        }

        composable(NavRoutes.BALANCE) {
            BalanceScreen(
                repository = repository,
                onNavigatePay = {
                    scannedPayTicket = null
                    navController.navigate(NavRoutes.PAY)
                },
                onNavigateReceive = {
                    scannedReceiveTokens = null
                    navController.navigate(NavRoutes.RECEIVE)
                },
                onNavigateTopUp = { navController.navigate(NavRoutes.TOP_UP) },
                onNavigateMyQr = { navController.navigate(NavRoutes.MY_QR) },
            )
        }

        composable(NavRoutes.MY_QR) {
            MyQrScreen(
                repository = repository,
                qrGenerator = qrGenerator,
                onBack = { navController.popBackStack() },
            )
        }

        composable(NavRoutes.PAY) {
            PayScreen(
                repository = repository,
                scannedTicketB64 = scannedPayTicket,
                onScanQr = { navController.navigate(NavRoutes.PAY_SCAN) },
                onBack = { navController.popBackStack() },
                onShowTransferQr = {
                    navController.popBackStack(NavRoutes.BALANCE, inclusive = false)
                },
            )
        }

        composable(NavRoutes.RECEIVE) {
            ReceiveScreen(
                repository = repository,
                scannedTokensB64 = scannedReceiveTokens,
                onScanQr = { navController.navigate(NavRoutes.RECEIVE_SCAN) },
                onBack = { navController.popBackStack() },
            )
        }

        composable(NavRoutes.TOP_UP) {
            TopUpScreen(
                repository = repository,
                onBack = { navController.popBackStack() },
            )
        }

        // QR Scanner screens — placeholder until CameraX + ML Kit integration
        composable(NavRoutes.PAY_SCAN) {
            ScannerPlaceholder(
                title = "Scan Recipient QR",
                onResult = { result ->
                    scannedPayTicket = result
                    navController.popBackStack()
                },
                onBack = { navController.popBackStack() },
            )
        }

        composable(NavRoutes.RECEIVE_SCAN) {
            ScannerPlaceholder(
                title = "Scan Transfer QR",
                onResult = { result ->
                    scannedReceiveTokens = listOf(result)
                    navController.popBackStack()
                },
                onBack = { navController.popBackStack() },
            )
        }
    }
}

/**
 * Placeholder for the QR scanner.
 *
 * In production, replace with CameraX + ML Kit barcode scanning.
 * For development/testing, allows pasting base64 data manually.
 */
@Composable
private fun ScannerPlaceholder(
    title: String,
    onResult: (String) -> Unit,
    onBack: () -> Unit,
) {
    var input by remember { mutableStateOf("") }

    Column(
        modifier = Modifier.fillMaxSize().padding(24.dp),
        horizontalAlignment = Alignment.CenterHorizontally,
    ) {
        Text(title, style = MaterialTheme.typography.titleLarge)
        Spacer(Modifier.height(16.dp))
        Text(
            "Camera scanner not yet integrated.\nPaste base64 data below for testing:",
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
