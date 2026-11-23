package com.briolette.pos.data

actual fun currentTimeMillis(): Long = System.currentTimeMillis()

actual fun ByteArray.toHexString(): String =
    joinToString("") { "%02x".format(it) }
