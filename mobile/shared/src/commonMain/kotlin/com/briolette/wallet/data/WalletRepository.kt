package com.briolette.wallet.data

import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow

/**
 * Bridge interface to the Rust FFI wallet operations.
 *
 * Platform-specific implementations (Android/iOS) call into the UniFFI-generated
 * Kotlin/Swift bindings. This interface allows the common UI layer to remain
 * platform-agnostic.
 */
interface WalletBridge {
    suspend fun createWallet(name: String, config: NetworkConfig): WalletState
    suspend fun loadWallet(json: String): WalletState
    suspend fun saveWallet(state: WalletState): String
    suspend fun synchronize(state: WalletState): WalletState
    suspend fun requestTickets(state: WalletState, count: Int): WalletState
    suspend fun withdraw(state: WalletState, amount: Int): WalletState
    suspend fun transfer(state: WalletState, recipientTicketB64: String, amount: Int): TransferResult
    suspend fun receiveTokens(state: WalletState, tokensB64: List<String>): WalletState
    suspend fun validate(state: WalletState): ValidationResult
    suspend fun getReceivingTicketB64(state: WalletState): String
}

/**
 * Central wallet repository managing state and coordinating operations.
 *
 * All UI screens observe [state] and call mutation methods. The repository
 * ensures operations are serialized (no concurrent mutations) and persists
 * state after each operation.
 */
class WalletRepository(
    private val bridge: WalletBridge,
    private val persistence: WalletPersistence,
) {
    private val _state = MutableStateFlow(WalletState())
    val state: StateFlow<WalletState> = _state.asStateFlow()

    private val _isLoading = MutableStateFlow(false)
    val isLoading: StateFlow<Boolean> = _isLoading.asStateFlow()

    private val _error = MutableStateFlow<String?>(null)
    val error: StateFlow<String?> = _error.asStateFlow()

    fun clearError() {
        _error.value = null
    }

    /** Try to load a previously saved wallet. */
    suspend fun tryLoadSaved(): Boolean {
        val json = persistence.load() ?: return false
        return try {
            _state.value = bridge.loadWallet(json)
            true
        } catch (e: Exception) {
            false
        }
    }

    /** Create a new wallet and register with the network. */
    suspend fun createWallet(name: String, config: NetworkConfig) {
        withLoading {
            val newState = bridge.createWallet(name, config)
            _state.value = newState
            persistence.save(newState.json)
        }
    }

    /** Sync epoch data from the clerk. */
    suspend fun synchronize() {
        withLoading {
            val updated = bridge.synchronize(_state.value)
            _state.value = updated
            persistence.save(updated.json)
        }
    }

    /** Request more receiving tickets. */
    suspend fun requestTickets(count: Int = 5) {
        withLoading {
            val updated = bridge.requestTickets(_state.value, count)
            _state.value = updated
            persistence.save(updated.json)
        }
    }

    /** Top up: withdraw tokens from the mint. */
    suspend fun topUp(amount: Int) {
        withLoading {
            val updated = bridge.withdraw(_state.value, amount)
            _state.value = updated
            persistence.save(updated.json)
        }
    }

    /** Pay: transfer tokens to a recipient's ticket. */
    suspend fun pay(recipientTicketB64: String, amount: Int): List<String> {
        return withLoading {
            val result = bridge.transfer(_state.value, recipientTicketB64, amount)
            _state.value = result.state
            persistence.save(result.state.json)
            result.tokensBase64
        }
    }

    /** Receive: import tokens from a sender. */
    suspend fun receiveTokens(tokensB64: List<String>) {
        withLoading {
            val updated = bridge.receiveTokens(_state.value, tokensB64)
            _state.value = updated
            persistence.save(updated.json)
        }
    }

    /** Get a base64-encoded receiving ticket for QR display. */
    suspend fun getReceivingTicket(): String {
        return bridge.getReceivingTicketB64(_state.value)
    }

    /** Validate all held tokens. */
    suspend fun validate(): ValidationResult {
        return withLoading {
            val result = bridge.validate(_state.value)
            _state.value = result.state
            persistence.save(result.state.json)
            result
        }
    }

    private suspend fun <T> withLoading(block: suspend () -> T): T {
        _isLoading.value = true
        _error.value = null
        return try {
            block()
        } catch (e: Exception) {
            _error.value = e.message ?: "Unknown error"
            throw e
        } finally {
            _isLoading.value = false
        }
    }
}

/**
 * Platform-specific wallet persistence (SharedPreferences on Android,
 * UserDefaults on iOS, etc.).
 */
interface WalletPersistence {
    suspend fun save(json: String)
    suspend fun load(): String?
    suspend fun clear()
}
