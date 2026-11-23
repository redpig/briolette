package com.briolette.wallet.ui.screens

import androidx.compose.foundation.layout.*
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.itemsIndexed
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import com.briolette.wallet.data.WalletRepository

/**
 * Transaction history / token inventory screen.
 *
 * Shows each token held in the wallet with its value, transfer chain
 * length (number of hops), and validation status. Provides a summary
 * of the wallet's token portfolio.
 */
@Composable
fun HistoryScreen(
    repository: WalletRepository,
    onBack: () -> Unit,
) {
    val state by repository.state.collectAsState()
    val isLoading by repository.isLoading.collectAsState()

    // Parse token details from wallet JSON
    val tokenEntries = remember(state.json) {
        parseTokenEntries(state.json)
    }

    Column(
        modifier = Modifier.fillMaxSize().padding(top = 16.dp),
    ) {
        // Top bar
        Row(
            modifier = Modifier.fillMaxWidth().padding(horizontal = 16.dp),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            TextButton(onClick = onBack) { Text("Back") }
            Spacer(Modifier.weight(1f))
            Text("Token Inventory", style = MaterialTheme.typography.titleMedium)
            Spacer(Modifier.weight(1f))
            Spacer(Modifier.width(48.dp))
        }

        Spacer(Modifier.height(8.dp))

        // Summary card
        Card(
            modifier = Modifier.fillMaxWidth().padding(horizontal = 16.dp),
            colors = CardDefaults.cardColors(
                containerColor = MaterialTheme.colorScheme.secondaryContainer,
            ),
        ) {
            Row(
                modifier = Modifier.fillMaxWidth().padding(16.dp),
                horizontalArrangement = Arrangement.SpaceEvenly,
            ) {
                SummaryItem("Tokens", "${state.balance.tokenCount}")
                SummaryItem("Balance", state.balance.displayAmount)
                SummaryItem("Tickets", "${state.ticketCount}")
                SummaryItem("Currency", state.balance.currency)
            }
        }

        Spacer(Modifier.height(12.dp))

        if (tokenEntries.isEmpty()) {
            Box(
                modifier = Modifier.fillMaxSize(),
                contentAlignment = Alignment.Center,
            ) {
                Column(horizontalAlignment = Alignment.CenterHorizontally) {
                    Text(
                        "No tokens yet",
                        style = MaterialTheme.typography.titleMedium,
                        color = MaterialTheme.colorScheme.onBackground.copy(alpha = 0.5f),
                    )
                    Spacer(Modifier.height(8.dp))
                    Text(
                        "Use Top Up to withdraw tokens from the mint",
                        style = MaterialTheme.typography.bodyMedium,
                        color = MaterialTheme.colorScheme.onBackground.copy(alpha = 0.4f),
                    )
                }
            }
        } else {
            LazyColumn(
                contentPadding = PaddingValues(horizontal = 16.dp, vertical = 8.dp),
                verticalArrangement = Arrangement.spacedBy(8.dp),
            ) {
                itemsIndexed(tokenEntries) { index, entry ->
                    TokenCard(index = index, entry = entry)
                }
            }
        }
    }
}

@Composable
private fun SummaryItem(label: String, value: String) {
    Column(horizontalAlignment = Alignment.CenterHorizontally) {
        Text(
            value,
            fontWeight = FontWeight.Bold,
            style = MaterialTheme.typography.titleMedium,
            color = MaterialTheme.colorScheme.onSecondaryContainer,
        )
        Text(
            label,
            style = MaterialTheme.typography.bodySmall,
            color = MaterialTheme.colorScheme.onSecondaryContainer.copy(alpha = 0.7f),
        )
    }
}

@Composable
private fun TokenCard(index: Int, entry: TokenDisplayEntry) {
    Card(
        modifier = Modifier.fillMaxWidth(),
        shape = RoundedCornerShape(12.dp),
    ) {
        Row(
            modifier = Modifier.fillMaxWidth().padding(16.dp),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            // Token number
            Surface(
                shape = RoundedCornerShape(8.dp),
                color = MaterialTheme.colorScheme.primaryContainer,
                modifier = Modifier.size(40.dp),
            ) {
                Box(contentAlignment = Alignment.Center) {
                    Text(
                        "#${index + 1}",
                        fontWeight = FontWeight.Bold,
                        color = MaterialTheme.colorScheme.onPrimaryContainer,
                    )
                }
            }

            Spacer(Modifier.width(12.dp))

            // Token details
            Column(modifier = Modifier.weight(1f)) {
                Text(
                    "${entry.wholeValue} ${entry.currencyCode}",
                    fontWeight = FontWeight.Bold,
                    style = MaterialTheme.typography.bodyLarge,
                )
                Text(
                    "${entry.historyLength} transfer${if (entry.historyLength != 1) "s" else ""}",
                    style = MaterialTheme.typography.bodySmall,
                    color = MaterialTheme.colorScheme.onSurface.copy(alpha = 0.6f),
                )
            }

            // Fractional value badge
            if (entry.fractionalValue > 0) {
                Surface(
                    shape = RoundedCornerShape(8.dp),
                    color = MaterialTheme.colorScheme.tertiaryContainer,
                ) {
                    Text(
                        ".${(entry.fractionalValue / 10_000).toString().padStart(2, '0')}",
                        modifier = Modifier.padding(horizontal = 8.dp, vertical = 4.dp),
                        style = MaterialTheme.typography.bodySmall,
                        color = MaterialTheme.colorScheme.onTertiaryContainer,
                    )
                }
            }
        }
    }
}

/**
 * Display model for a single token in the inventory.
 */
data class TokenDisplayEntry(
    val wholeValue: Int,
    val fractionalValue: Int,
    val currencyCode: String,
    val historyLength: Int,
)

/**
 * Parse token entries from wallet JSON for display.
 */
private fun parseTokenEntries(json: String): List<TokenDisplayEntry> {
    if (json.isBlank()) return emptyList()
    return try {
        // Use a simple JSON parser approach for KMP compatibility
        val startIdx = json.indexOf("\"tokens\"")
        if (startIdx == -1) return emptyList()

        // Extract the tokens array
        // This is a simplified parser; in production use kotlinx.serialization
        val entries = mutableListOf<TokenDisplayEntry>()

        // Find each token entry by looking for whole_value patterns
        var searchFrom = startIdx
        while (true) {
            val wholeIdx = json.indexOf("\"whole_value\"", searchFrom)
            if (wholeIdx == -1) break

            val wholeValue = extractIntAfterColon(json, wholeIdx) ?: 0
            val fracIdx = json.indexOf("\"fractional_value\"", wholeIdx)
            val fracValue = if (fracIdx != -1 && fracIdx - wholeIdx < 100) {
                extractFloatAfterColon(json, fracIdx)?.toInt() ?: 0
            } else 0

            val codeIdx = json.indexOf("\"value_code\"", wholeIdx)
            val code = if (codeIdx != -1 && codeIdx - wholeIdx < 150) {
                extractIntAfterColon(json, codeIdx) ?: 0
            } else 0

            val currencyCode = when (code) {
                0 -> "TEST"
                840 -> "USD"
                978 -> "EUR"
                8888 -> "ETH"
                else -> "CODE_$code"
            }

            entries.add(
                TokenDisplayEntry(
                    wholeValue = wholeValue,
                    fractionalValue = fracValue,
                    currencyCode = currencyCode,
                    historyLength = 0, // Would need token protobuf decoding
                )
            )

            searchFrom = wholeIdx + 20
        }

        entries
    } catch (_: Exception) {
        emptyList()
    }
}

private fun extractIntAfterColon(json: String, keyIdx: Int): Int? {
    val colonIdx = json.indexOf(':', keyIdx)
    if (colonIdx == -1) return null
    val numStart = json.indexOfFirst(colonIdx + 1) { it.isDigit() || it == '-' }
    if (numStart == -1) return null
    val numEnd = json.indexOfFirst(numStart + 1) { !it.isDigit() && it != '-' }
    return json.substring(numStart, if (numEnd == -1) json.length else numEnd).toIntOrNull()
}

private fun extractFloatAfterColon(json: String, keyIdx: Int): Float? {
    val colonIdx = json.indexOf(':', keyIdx)
    if (colonIdx == -1) return null
    val numStart = json.indexOfFirst(colonIdx + 1) { it.isDigit() || it == '-' || it == '.' }
    if (numStart == -1) return null
    val numEnd = json.indexOfFirst(numStart + 1) { !it.isDigit() && it != '.' && it != '-' }
    return json.substring(numStart, if (numEnd == -1) json.length else numEnd).toFloatOrNull()
}

private inline fun String.indexOfFirst(startIndex: Int, predicate: (Char) -> Boolean): Int {
    for (i in startIndex until length) {
        if (predicate(this[i])) return i
    }
    return -1
}
