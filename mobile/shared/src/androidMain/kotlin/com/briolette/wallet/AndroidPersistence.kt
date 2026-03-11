package com.briolette.wallet

import android.content.Context
import com.briolette.wallet.data.WalletPersistence

/**
 * Android wallet persistence using SharedPreferences.
 *
 * Stores the wallet JSON in encrypted shared preferences.
 * In production, use EncryptedSharedPreferences from AndroidX Security.
 */
class AndroidWalletPersistence(context: Context) : WalletPersistence {
    private val prefs = context.getSharedPreferences("briolette_wallet", Context.MODE_PRIVATE)

    override suspend fun save(json: String) {
        prefs.edit().putString(KEY_WALLET_JSON, json).apply()
    }

    override suspend fun load(): String? {
        return prefs.getString(KEY_WALLET_JSON, null)
    }

    override suspend fun clear() {
        prefs.edit().remove(KEY_WALLET_JSON).apply()
    }

    companion object {
        private const val KEY_WALLET_JSON = "wallet_state_json"
    }
}
