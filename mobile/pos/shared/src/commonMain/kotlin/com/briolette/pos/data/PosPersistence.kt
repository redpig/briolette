package com.briolette.pos.data

/**
 * Persistence interface for PoS state and token storage.
 *
 * Platform implementations use:
 * - Android: Room/SQLite for token store, SharedPreferences for config
 * - iOS: CoreData/SQLite for token store, UserDefaults for config
 */
interface PosPersistence {
    /** Save terminal state (merchant ticket, epoch, totals). */
    fun saveState(state: PosState)

    /** Load terminal state. Returns null if not configured. */
    fun loadState(): PosState?

    /** Save a payment record and its token data. */
    fun savePayment(record: PaymentRecord, tokenData: ByteArray)

    /** Load recent payment records (most recent first). */
    fun loadRecentPayments(limit: Int = 50): List<PaymentRecord>

    /** Load token data for a specific payment. */
    fun loadTokenData(paymentId: String): ByteArray

    /** Load all unswept token data for merchant collection. */
    fun loadUnsweptTokens(): ByteArray

    /** Mark all current tokens as swept. */
    fun markSwept()

    /** Load payments pending online validation. */
    fun loadPendingValidation(): List<PaymentRecord>

    /** Update validation status for a payment. */
    fun updateValidationStatus(paymentId: String, status: ValidationStatus)
}
