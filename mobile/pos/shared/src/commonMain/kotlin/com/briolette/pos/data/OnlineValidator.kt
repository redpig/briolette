package com.briolette.pos.data

/**
 * Online token validation client.
 *
 * When the PoS has internet connectivity, it validates tokens against
 * the Briolette network:
 * - Tokenmap: Check for double-spend (token already transferred)
 * - Clerk: Epoch gossip (stay current with network state)
 * - Validate: Cryptographic verification (BLS pairing checks)
 *
 * The PoS gracefully degrades when offline — it accepts tokens on
 * faith and validates them later when connectivity returns.
 */
interface OnlineValidator {
    /**
     * Check tokens against the tokenmap and validator services.
     *
     * @param tokenData Serialized token data (unsigned or signed)
     * @return Validation result with crypto and online checks
     */
    suspend fun checkTokens(tokenData: ByteArray): TokenValidationResult

    /**
     * Sync epoch data with the Clerk service.
     *
     * @return Updated epoch data (serialized protobuf), or null if offline
     */
    suspend fun syncEpoch(): EpochSyncResult?

    /**
     * Check if the validator has network connectivity.
     */
    suspend fun isOnline(): Boolean
}

data class TokenValidationResult(
    /** Whether the token chain is cryptographically valid (BLS pairing checks). */
    val cryptoValid: Boolean,
    /** Whether the tokenmap confirms the tokens are unspent. Null if offline. */
    val onlineValid: Boolean?,
    /** Details about any validation failures. */
    val details: String = "",
)

data class EpochSyncResult(
    val epochDataB64: String,
    val epochNumber: Int,
    val updated: Boolean,
)
