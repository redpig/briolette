package com.briolette.pos.data

/**
 * APDU protocol for PoS ↔ credstick communication.
 *
 * Mirrors receiver.proto RPCs over ISO-DEP NFC:
 * - INITIATE (0x10): Send ticket + items + epoch to credstick
 * - READ_TICKET (0x11): Read credstick's SignedTicket (setup/sweep)
 * - GOSSIP (0x12): Exchange epoch updates
 * - TRANSACT (0x20): Receive unsigned token proposal from credstick
 * - TRANSFER (0x30): Send accept/reject, receive signatures
 * - RECEIVE (0x31): Deliver signed tokens to receiving credstick
 */
object ApduProtocol {

    /** Briolette AID (matches JavaCard applet). */
    val BRIOLETTE_AID = byteArrayOf(
        0xA0.toByte(), 0x00, 0x00, 0x00, 0x62, 0x03, 0x01
    )

    // CLA byte for all Briolette commands.
    private const val CLA: Byte = 0x80.toByte()

    // INS bytes — mirrors receiver.proto.
    object Ins {
        const val INITIATE: Byte = 0x10
        const val READ_TICKET: Byte = 0x11
        const val GOSSIP: Byte = 0x12
        const val TRANSACT: Byte = 0x20
        const val TRANSFER: Byte = 0x30
        const val RECEIVE: Byte = 0x31
        const val SWEEP: Byte = 0x50
        const val GET_BALANCE: Byte = 0x51
    }

    // Status words.
    object Sw {
        const val SUCCESS: Int = 0x9000
        const val PIN_REQUIRED_BASE: Int = 0x63C0  // 0x63CX where X = retries
        const val CONDITIONS_NOT_SATISFIED: Int = 0x6985
        const val LOCKED: Int = 0x6983

        fun isSuccess(sw: Int): Boolean = sw == SUCCESS
        fun isPinRequired(sw: Int): Boolean = (sw and 0xFFF0) == PIN_REQUIRED_BASE
        fun pinRetries(sw: Int): Int = sw and 0x0F
    }

    /**
     * Build a SELECT command for the Briolette AID.
     */
    fun selectApdu(): ByteArray {
        // SELECT by DF name: CLA=00 INS=A4 P1=04 P2=00 Lc=07 [AID]
        return byteArrayOf(
            0x00, 0xA4.toByte(), 0x04, 0x00,
            BRIOLETTE_AID.size.toByte(),
            *BRIOLETTE_AID
        )
    }

    /**
     * Build an INITIATE APDU.
     *
     * Sends the payment proposal to the credstick:
     * - Amount (4 bytes big-endian whole + 4 bytes micros)
     * - Description (UTF-8, up to 32 bytes)
     * - Ticket data (serialized SignedTicket protobuf)
     * - Epoch data (serialized EpochData protobuf)
     *
     * The credstick displays the amount on its e-ink screen and
     * returns either unsigned tokens (2-tap) or just a tx_id (3-tap).
     */
    fun initiateApdu(
        amount: Int,
        description: String,
        ticketData: ByteArray,
        epochData: ByteArray,
    ): ByteArray {
        val descBytes = description.toByteArray(Charsets.UTF_8).take(32).toByteArray()

        // Payload: [4B amount][4B micros=0][desc bytes][ticket][epoch]
        val data = ByteArray(8 + descBytes.size + ticketData.size + epochData.size)
        data[0] = (amount shr 24).toByte()
        data[1] = (amount shr 16).toByte()
        data[2] = (amount shr 8).toByte()
        data[3] = amount.toByte()
        // Micros = 0 (data[4..7])
        descBytes.copyInto(data, 8)
        ticketData.copyInto(data, 8 + descBytes.size)
        epochData.copyInto(data, 8 + descBytes.size + ticketData.size)

        return buildApdu(Ins.INITIATE, data = data)
    }

    /**
     * Build a READ_TICKET APDU.
     * Returns the credstick's SignedTicket (used during setup and sweep).
     */
    fun readTicketApdu(): ByteArray {
        return buildApdu(Ins.READ_TICKET)
    }

    /**
     * Build a GOSSIP APDU for epoch exchange.
     */
    fun gossipApdu(epochData: ByteArray): ByteArray {
        return buildApdu(Ins.GOSSIP, data = epochData)
    }

    /**
     * Build a TRANSACT APDU.
     * In 3-tap mode, this requests unsigned tokens from the credstick.
     * In 2-tap mode, tokens were already returned with INITIATE.
     */
    fun transactApdu(txId: ByteArray): ByteArray {
        return buildApdu(Ins.TRANSACT, data = txId)
    }

    /**
     * Build a TRANSFER APDU.
     *
     * Tells the credstick to sign and commit (accept=true) or abort (accept=false).
     * If accepted, the credstick returns BLS signatures for the proposed tokens.
     */
    fun transferApdu(txId: ByteArray, accept: Boolean): ByteArray {
        val data = ByteArray(txId.size + 1)
        txId.copyInto(data)
        data[txId.size] = if (accept) 0x01 else 0x00
        return buildApdu(Ins.TRANSFER, data = data)
    }

    /**
     * Build a RECEIVE APDU.
     * Delivers signed tokens to a receiving credstick.
     */
    fun receiveApdu(signedTokens: ByteArray): ByteArray {
        return buildApdu(Ins.RECEIVE, data = signedTokens)
    }

    /**
     * Build a SWEEP APDU.
     * Collects accumulated tokens from a PoS credstick.
     */
    fun sweepApdu(tokens: ByteArray): ByteArray {
        return buildApdu(Ins.SWEEP, data = tokens)
    }

    /**
     * Build a GET_BALANCE APDU.
     */
    fun getBalanceApdu(): ByteArray {
        return buildApdu(Ins.GET_BALANCE)
    }

    /**
     * Extract status word from an APDU response.
     * SW is the last 2 bytes.
     */
    fun extractSw(response: ByteArray): Int {
        if (response.size < 2) return 0
        val sw1 = response[response.size - 2].toInt() and 0xFF
        val sw2 = response[response.size - 1].toInt() and 0xFF
        return (sw1 shl 8) or sw2
    }

    /**
     * Extract data from an APDU response (everything except SW).
     */
    fun extractData(response: ByteArray): ByteArray {
        if (response.size <= 2) return ByteArray(0)
        return response.copyOfRange(0, response.size - 2)
    }

    // --- Internal ---

    private fun buildApdu(
        ins: Byte,
        p1: Byte = 0x00,
        p2: Byte = 0x00,
        data: ByteArray = ByteArray(0),
    ): ByteArray {
        return if (data.isEmpty()) {
            byteArrayOf(CLA, ins, p1, p2)
        } else {
            byteArrayOf(CLA, ins, p1, p2, data.size.toByte(), *data)
        }
    }
}
