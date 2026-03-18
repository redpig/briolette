// Copyright 2024 Google LLC
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     https://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

package com.briolette.javacard;

import javacard.framework.Util;
import javacard.security.MessageDigest;

/**
 * Bloom filter for client-side double-spend prevention.
 *
 * Tracks spent basenames (G1 points used as basename_base in ECDAA signing)
 * to prevent the card from signing with the same basename twice within an epoch.
 * This is defense-in-depth — the network still performs authoritative
 * double-spend detection via basename linkability in the tokenmap.
 *
 * Parameters (tuned for ~1000 basenames/epoch, 1% false positive rate):
 *   - Bit array: 9585 bits = 1199 bytes (rounded up to 1200)
 *   - Hash functions: 7
 *   - Hash: SHA-256 of the basename G1 point (65 bytes -> 32 bytes -> 7 positions)
 */
public class BloomFilter {
    /** Number of bits in the bloom filter. */
    private static final short NUM_BITS = (short) 9585;

    /** Number of bytes in the bloom filter storage. */
    public static final short STORAGE_BYTES = (short) 1200;

    /** Number of hash functions (bit positions per element). */
    private static final byte NUM_HASHES = 7;

    /** The bit array stored in EEPROM. */
    private byte[] bits;

    /** SHA-256 digest instance for hashing basenames. */
    private MessageDigest sha256;

    /** Scratch buffer for hash output (transient RAM). */
    private byte[] hashBuf;

    /** Current epoch counter (monotonically increasing). */
    private int epochCounter;

    public BloomFilter() {
        bits = new byte[STORAGE_BYTES];
        sha256 = MessageDigest.getInstance(MessageDigest.ALG_SHA_256, false);
        // Use transient memory for hash buffer to avoid EEPROM wear
        hashBuf = javacard.framework.JCSystem.makeTransientByteArray(
            (short) 32, javacard.framework.JCSystem.CLEAR_ON_DESELECT);
        epochCounter = 0;
    }

    /**
     * Check if a basename has been seen before and, if not, add it.
     *
     * @param basenamePoint The serialized G1 point (65 bytes for BN254).
     * @param offset        Offset into the buffer.
     * @param length        Length of the basename data.
     * @return true if the basename was already present (probable double-spend),
     *         false if it was not present (and has now been added).
     */
    public boolean checkAndAdd(byte[] basenamePoint, short offset, short length) {
        // Hash the basename point
        sha256.reset();
        sha256.doFinal(basenamePoint, offset, length, hashBuf, (short) 0);

        // Derive 7 bit positions from the 32-byte hash.
        // Each position uses 2 bytes from the hash, mod NUM_BITS.
        boolean allSet = true;
        for (byte i = 0; i < NUM_HASHES; i++) {
            short hashIdx = (short) (i * 2);
            // Read 2 bytes as unsigned 16-bit value
            int pos = ((hashBuf[hashIdx] & 0xFF) << 8) | (hashBuf[(short)(hashIdx + 1)] & 0xFF);
            // Mod by number of bits (positive since pos is unsigned)
            pos = pos % NUM_BITS;

            short byteIdx = (short) (pos / 8);
            byte bitMask = (byte) (1 << (pos % 8));

            if ((bits[byteIdx] & bitMask) == 0) {
                allSet = false;
            }
        }

        if (allSet) {
            // All bits were set — probable duplicate
            return true;
        }

        // Not a duplicate: set all bits
        for (byte i = 0; i < NUM_HASHES; i++) {
            short hashIdx = (short) (i * 2);
            int pos = ((hashBuf[hashIdx] & 0xFF) << 8) | (hashBuf[(short)(hashIdx + 1)] & 0xFF);
            pos = pos % NUM_BITS;

            short byteIdx = (short) (pos / 8);
            byte bitMask = (byte) (1 << (pos % 8));
            bits[byteIdx] |= bitMask;
        }

        return false;
    }

    /**
     * Reset the bloom filter for a new epoch.
     * Only succeeds if newEpoch > current epoch (monotonic).
     *
     * @param newEpoch The new epoch counter value (big-endian u32 from APDU data).
     * @return true if reset succeeded, false if newEpoch is not strictly greater.
     */
    public boolean resetForEpoch(int newEpoch) {
        if (newEpoch <= epochCounter) {
            return false;
        }
        epochCounter = newEpoch;
        Util.arrayFillNonAtomic(bits, (short) 0, STORAGE_BYTES, (byte) 0);
        return true;
    }

    /**
     * Get the current epoch counter.
     */
    public int getEpoch() {
        return epochCounter;
    }
}
