package com.briolette.pos

import android.content.Context
import android.content.SharedPreferences
import com.briolette.pos.data.PaymentRecord
import com.briolette.pos.data.PosPersistence
import com.briolette.pos.data.PosState
import com.briolette.pos.data.ValidationStatus

/**
 * Android persistence using SharedPreferences for config and
 * a simple file-based store for token data.
 *
 * Production should use Room/SQLite for the token store (matching
 * the schema in pos-relay-app.md).
 */
class AndroidPosPersistence(context: Context) : PosPersistence {

    private val prefs: SharedPreferences =
        context.getSharedPreferences("briolette_pos", Context.MODE_PRIVATE)

    override fun saveState(state: PosState) {
        prefs.edit()
            .putBoolean("merchant_configured", state.merchantConfigured)
            .putString("merchant_ticket", state.merchantTicketB64)
            .putString("epoch_data", state.epochDataB64)
            .putInt("epoch_number", state.epochNumber)
            .putInt("total_accumulated", state.totalAccumulated)
            .putInt("validated_count", state.validatedCount)
            .putInt("unvalidated_count", state.unvalidatedCount)
            .apply()
    }

    override fun loadState(): PosState? {
        if (!prefs.getBoolean("merchant_configured", false)) return null
        return PosState(
            merchantConfigured = true,
            merchantTicketB64 = prefs.getString("merchant_ticket", "") ?: "",
            epochDataB64 = prefs.getString("epoch_data", "") ?: "",
            epochNumber = prefs.getInt("epoch_number", 0),
            totalAccumulated = prefs.getInt("total_accumulated", 0),
            validatedCount = prefs.getInt("validated_count", 0),
            unvalidatedCount = prefs.getInt("unvalidated_count", 0),
        )
    }

    override fun savePayment(record: PaymentRecord, tokenData: ByteArray) {
        // TODO: Use Room/SQLite. For now, accumulate in SharedPreferences.
        val count = prefs.getInt("payment_count", 0)
        prefs.edit()
            .putString("payment_${count}_id", record.id)
            .putLong("payment_${count}_ts", record.timestamp)
            .putInt("payment_${count}_amount", record.amount)
            .putString("payment_${count}_desc", record.description)
            .putString("payment_${count}_status", record.validationStatus.name)
            .putBoolean("payment_${count}_swept", false)
            .putInt("payment_count", count + 1)
            .apply()

        // Token data stored separately (could be large).
        // TODO: Store in files or SQLite BLOB.
    }

    override fun loadRecentPayments(limit: Int): List<PaymentRecord> {
        val count = prefs.getInt("payment_count", 0)
        val start = maxOf(0, count - limit)
        return (start until count).reversed().map { i ->
            PaymentRecord(
                id = prefs.getString("payment_${i}_id", "") ?: "",
                timestamp = prefs.getLong("payment_${i}_ts", 0),
                amount = prefs.getInt("payment_${i}_amount", 0),
                description = prefs.getString("payment_${i}_desc", "") ?: "",
                validationStatus = try {
                    ValidationStatus.valueOf(
                        prefs.getString("payment_${i}_status", "PENDING") ?: "PENDING"
                    )
                } catch (_: Exception) { ValidationStatus.PENDING },
                swept = prefs.getBoolean("payment_${i}_swept", false),
            )
        }
    }

    override fun loadTokenData(paymentId: String): ByteArray {
        // TODO: Load from file/SQLite.
        return ByteArray(0)
    }

    override fun loadUnsweptTokens(): ByteArray {
        // TODO: Aggregate all unswept token data.
        return ByteArray(0)
    }

    override fun markSwept() {
        val count = prefs.getInt("payment_count", 0)
        val editor = prefs.edit()
        for (i in 0 until count) {
            editor.putBoolean("payment_${i}_swept", true)
        }
        editor.apply()
    }

    override fun loadPendingValidation(): List<PaymentRecord> {
        return loadRecentPayments().filter {
            it.validationStatus == ValidationStatus.PENDING && !it.swept
        }
    }

    override fun updateValidationStatus(paymentId: String, status: ValidationStatus) {
        val count = prefs.getInt("payment_count", 0)
        for (i in 0 until count) {
            if (prefs.getString("payment_${i}_id", "") == paymentId) {
                prefs.edit().putString("payment_${i}_status", status.name).apply()
                break
            }
        }
    }
}
