package com.briolette.pos

import android.os.Bundle
import androidx.activity.ComponentActivity
import androidx.activity.compose.setContent
import androidx.compose.material3.MaterialTheme
import com.briolette.pos.data.PosRepository
import com.briolette.pos.ui.PosApp

/**
 * PoS terminal main activity.
 *
 * A single-purpose merchant terminal app. Simpler than the wallet app:
 * no key generation, no token storage, no QR codes. Just:
 * 1. Register merchant credstick (one-time setup)
 * 2. Enter amount, tap customer credstick (2 taps)
 * 3. Accumulate tokens, sweep to merchant credstick periodically
 */
class MainActivity : ComponentActivity() {

    private lateinit var repository: PosRepository

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)

        // Initialize repository with platform-specific implementations.
        // TODO: Use Koin DI for proper injection.
        val persistence = AndroidPosPersistence(this)
        val onlineValidator = null // TODO: gRPC client when online
        repository = PosRepository(persistence, onlineValidator)

        setContent {
            MaterialTheme {
                PosApp(repository)
            }
        }
    }
}
