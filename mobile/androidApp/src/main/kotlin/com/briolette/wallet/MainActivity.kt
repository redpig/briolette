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
import com.briolette.wallet.data.NetworkConfig
import com.briolette.wallet.data.WalletRepository
import com.briolette.wallet.navigation.NavRoutes
import com.briolette.wallet.ui.scanner.QrScannerScreen
import com.briolette.wallet.ui.screens.*
import com.briolette.wallet.ui.theme.BrioletteTheme
import org.json.JSONArray
import org.json.JSONObject

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

    // Shared state between screens
    var scannedPayTicket by remember { mutableStateOf<String?>(null) }
    var scannedReceiveTokens by remember { mutableStateOf<List<String>?>(null) }
    var outgoingTokens by remember { mutableStateOf<List<String>>(emptyList()) }
    var networkConfig by remember { mutableStateOf(NetworkConfig()) }

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

        // ---- Setup ----
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

        // ---- Home / Balance ----
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
                onNavigateHistory = { navController.navigate(NavRoutes.HISTORY) },
                onNavigateSettings = { navController.navigate(NavRoutes.SETTINGS) },
            )
        }

        // ---- My QR (receiving address) ----
        composable(NavRoutes.MY_QR) {
            MyQrScreen(
                repository = repository,
                qrGenerator = qrGenerator,
                onBack = { navController.popBackStack() },
            )
        }

        // ---- Pay ----
        composable(NavRoutes.PAY) {
            PayScreen(
                repository = repository,
                scannedTicketB64 = scannedPayTicket,
                onScanQr = { navController.navigate(NavRoutes.PAY_SCAN) },
                onBack = { navController.popBackStack() },
                onShowTransferQr = { tokens ->
                    outgoingTokens = tokens
                    navController.navigate(NavRoutes.TRANSFER_QR)
                },
            )
        }

        // ---- Pay: scan recipient's QR code ----
        composable(NavRoutes.PAY_SCAN) {
            QrScannerScreen(
                title = "Scan Recipient",
                onResult = { result ->
                    // The scanned QR contains a base64 SignedTicket.
                    // It may be raw base64 or wrapped in a JSON envelope.
                    scannedPayTicket = extractTicketFromQr(result)
                    navController.popBackStack()
                },
                onBack = { navController.popBackStack() },
            )
        }

        // ---- Transfer QR (show tokens for recipient to scan) ----
        composable(NavRoutes.TRANSFER_QR) {
            TransferQrScreen(
                tokensBase64 = outgoingTokens,
                qrGenerator = qrGenerator,
                onDone = {
                    navController.popBackStack(NavRoutes.BALANCE, inclusive = false)
                },
            )
        }

        // ---- Receive ----
        composable(NavRoutes.RECEIVE) {
            ReceiveScreen(
                repository = repository,
                scannedTokensB64 = scannedReceiveTokens,
                onScanQr = { navController.navigate(NavRoutes.RECEIVE_SCAN) },
                onBack = { navController.popBackStack() },
            )
        }

        // ---- Receive: scan sender's transfer QR ----
        composable(NavRoutes.RECEIVE_SCAN) {
            QrScannerScreen(
                title = "Scan Payment",
                onResult = { result ->
                    scannedReceiveTokens = extractTokensFromQr(result)
                    navController.popBackStack()
                },
                onBack = { navController.popBackStack() },
            )
        }

        // ---- History / Token Inventory ----
        composable(NavRoutes.HISTORY) {
            HistoryScreen(
                repository = repository,
                onBack = { navController.popBackStack() },
            )
        }

        // ---- Top Up ----
        composable(NavRoutes.TOP_UP) {
            TopUpScreen(
                repository = repository,
                onBack = { navController.popBackStack() },
            )
        }

        // ---- Settings ----
        composable(NavRoutes.SETTINGS) {
            SettingsScreen(
                repository = repository,
                currentConfig = networkConfig,
                onConfigChanged = { networkConfig = it },
                onBack = { navController.popBackStack() },
                onResetWallet = {
                    navController.navigate(NavRoutes.SETUP) {
                        popUpTo(NavRoutes.BALANCE) { inclusive = true }
                    }
                },
            )
        }
    }
}

/**
 * Parse a scanned QR code to extract a base64 SignedTicket.
 *
 * Handles two formats:
 *   - Raw base64 string (from MyQrScreen)
 *   - JSON envelope: {"type":"ticket","data":"<base64>"}
 */
private fun extractTicketFromQr(raw: String): String {
    return try {
        val json = JSONObject(raw)
        json.getString("data")
    } catch (_: Exception) {
        // Assume raw base64
        raw.trim()
    }
}

/**
 * Parse a scanned QR code to extract base64-encoded tokens.
 *
 * Handles:
 *   - Single token: {"type":"token","data":"<base64>"}
 *   - Multi token: {"type":"transfer","tokens":["<b64>","<b64>",...]}
 *   - Raw base64 (single token fallback)
 */
private fun extractTokensFromQr(raw: String): List<String> {
    return try {
        val json = JSONObject(raw)
        when (json.optString("type")) {
            "token" -> listOf(json.getString("data"))
            "transfer" -> {
                val arr = json.getJSONArray("tokens")
                (0 until arr.length()).map { arr.getString(it) }
            }
            else -> listOf(raw.trim())
        }
    } catch (_: Exception) {
        listOf(raw.trim())
    }
}
