package com.briolette.pos

import com.briolette.pos.data.NfcTag
import com.briolette.pos.data.NfcTerminal
import kotlinx.coroutines.suspendCancellableCoroutine
import kotlin.coroutines.resume

/**
 * Android NFC reader mode implementation.
 *
 * Uses Android's IsoDep (ISO 14443-4 / ISO-DEP) to communicate with
 * credsticks. The phone acts as an NFC reader (initiator), sending APDUs
 * to the credstick which acts as a tag (responder).
 *
 * Required permissions in AndroidManifest.xml:
 * - android.permission.NFC
 *
 * Required features:
 * - android.hardware.nfc
 */
class AndroidNfcTerminal : NfcTerminal {

    override val isAvailable: Boolean
        get() {
            // Check NfcAdapter.getDefaultAdapter(context) != null.
            // TODO: requires Activity context.
            return true
        }

    override suspend fun waitForTag(): NfcTag? {
        // Android NFC reader mode:
        //
        // 1. Get NfcAdapter from context
        // 2. Enable reader mode with FLAG_READER_NFC_A | FLAG_READER_SKIP_NDEF_CHECK
        // 3. In the callback, get IsoDep from the discovered tag
        // 4. Connect and wrap in our NfcTag interface
        //
        // Example implementation:
        //
        // return suspendCancellableCoroutine { cont ->
        //     val adapter = NfcAdapter.getDefaultAdapter(context)
        //     adapter.enableReaderMode(activity, { tag ->
        //         val isoDep = IsoDep.get(tag)
        //         if (isoDep != null) {
        //             isoDep.connect()
        //             isoDep.timeout = 5000 // 5 seconds for signing
        //             cont.resume(AndroidNfcTag(isoDep))
        //         } else {
        //             cont.resume(null)
        //         }
        //     }, NfcAdapter.FLAG_READER_NFC_A or NfcAdapter.FLAG_READER_SKIP_NDEF_CHECK, null)
        //
        //     cont.invokeOnCancellation {
        //         adapter.disableReaderMode(activity)
        //     }
        // }

        // TODO: implement with Activity context injection.
        return null
    }

    override suspend fun stopReading() {
        // adapter.disableReaderMode(activity)
    }
}

/**
 * Wrapper around Android IsoDep for our NfcTag interface.
 */
class AndroidNfcTag(
    // private val isoDep: android.nfc.tech.IsoDep
) : NfcTag {

    override suspend fun transceive(apdu: ByteArray): ByteArray {
        // return isoDep.transceive(apdu)
        return ByteArray(0) // TODO: implement
    }

    override val isConnected: Boolean
        get() = false // isoDep.isConnected

    override suspend fun close() {
        // isoDep.close()
    }
}
