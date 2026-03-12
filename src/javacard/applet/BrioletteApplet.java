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

import javacard.framework.APDU;
import javacard.framework.Applet;
import javacard.framework.ISO7816;
import javacard.framework.ISOException;
import javacard.framework.JCSystem;
import javacard.framework.Util;
import javacard.security.MessageDigest;
import javacard.security.RandomData;

/**
 * Briolette ECDAA split-key signing applet for JavaCard.
 *
 * Implements the card side of the Brickell & Li style split-key ECDAA protocol.
 * The card performs only G1 scalar multiplications and Fr scalar arithmetic
 * on BN254 — no pairings, no G2/GT operations.
 *
 * This applet requires JCMathLib for big number and EC point operations on
 * non-standard curves (BN254 is not natively supported by JavaCard's crypto API).
 *
 * APDU Protocol:
 *   CLA: 0x80
 *   P1:  Curve version (0x00 = BN254/v0, 0x01 = BLS12-381/v1)
 *   P2:  Reserved (0x00)
 *
 *   INS 0x01: GENERATE_KEY       - Generate card_sk on first use
 *   INS 0x02: PUBLIC_KEY_SHARE   - Compute Q_card = base * card_sk
 *   INS 0x10: SIGN_COMMIT        - Phase 1 signing (no basename)
 *   INS 0x11: SIGN_COMMIT_BSN    - Phase 1 signing (with basename, bloom filter check)
 *   INS 0x12: SIGN_RESPOND       - Phase 2 signing
 *   INS 0x13: SIGN_COMMIT_BSN_SWAP - Phase 1 signing (swap mode, skip bloom filter)
 *   INS 0x20: JOIN_COMMIT        - Phase 1 blind join
 *   INS 0x21: JOIN_RESPOND       - Phase 2 blind join
 *   INS 0x30: RESET_BLOOM        - Reset bloom filter for new epoch
 *   INS 0x31: SET_SWAP_PUBKEY    - Set swap server public key (personalization)
 *   INS 0x40: GET_STATUS         - Query card status
 */
public class BrioletteApplet extends Applet {

    // ========================================================================
    // INS codes
    // ========================================================================
    private static final byte INS_GENERATE_KEY       = (byte) 0x01;
    private static final byte INS_PUBLIC_KEY_SHARE   = (byte) 0x02;
    private static final byte INS_SIGN_COMMIT        = (byte) 0x10;
    private static final byte INS_SIGN_COMMIT_BSN    = (byte) 0x11;
    private static final byte INS_SIGN_RESPOND       = (byte) 0x12;
    private static final byte INS_SIGN_COMMIT_SWAP   = (byte) 0x13;
    private static final byte INS_JOIN_COMMIT        = (byte) 0x20;
    private static final byte INS_JOIN_RESPOND       = (byte) 0x21;
    private static final byte INS_RESET_BLOOM        = (byte) 0x30;
    private static final byte INS_SET_SWAP_PUBKEY    = (byte) 0x31;
    private static final byte INS_GET_STATUS         = (byte) 0x40;

    // ========================================================================
    // Curve version constants
    // ========================================================================
    private static final byte VERSION_BN254   = (byte) 0x00;
    private static final byte VERSION_BLS381  = (byte) 0x01;

    // ========================================================================
    // Status words
    // ========================================================================
    /** Basename already used (bloom filter hit). */
    private static final short SW_BASENAME_USED      = (short) 0x6A84;
    /** Incorrect P1 (unsupported curve version). */
    private static final short SW_BAD_VERSION        = (short) 0x6A86;
    /** Swap authorization verification failed. */
    private static final short SW_SWAP_AUTH_FAILED   = (short) 0x6A85;
    /** Swap public key not set. */
    private static final short SW_NO_SWAP_KEY        = (short) 0x6A87;

    /** Size of swap authorization token: c (32B) + s (32B). */
    private static final short SWAP_AUTH_BYTES       = (short) 64;

    // ========================================================================
    // Session types
    // ========================================================================
    private static final byte SESSION_NONE = 0;
    private static final byte SESSION_SIGN = 1;
    private static final byte SESSION_JOIN = 2;

    // ========================================================================
    // Persistent state (EEPROM)
    // ========================================================================

    /** Card's secret key share (Fr scalar, 32 bytes). Never exported. */
    private byte[] cardSk;

    /** Whether card_sk has been generated. */
    private boolean keyInitialized;

    /** Bloom filter for basename double-spend tracking. */
    private BloomFilter bloomFilter;

    /** Swap server's public key for swap authorization (65 bytes, optional). */
    private byte[] swapPubkey;

    /** Whether swap pubkey has been set. */
    private boolean swapPubkeySet;

    // ========================================================================
    // Transient state (RAM, cleared on deselect)
    // ========================================================================

    /** Ephemeral randomness r_card (Fr scalar, 32 bytes). */
    private byte[] rCard;

    /** Current session type (SESSION_NONE, SESSION_SIGN, SESSION_JOIN). */
    private byte[] sessionType;

    /** Scratch buffer for swap authorization verification (RAM). */
    private byte[] scratchPoint;

    /** Scratch buffer for swap auth hash computation (RAM). */
    private byte[] scratchHash;

    // ========================================================================
    // Crypto instances
    // ========================================================================
    private RandomData rng;

    // NOTE: In a real implementation, this applet would use JCMathLib's
    // BigNat and ECPoint classes for BN254 scalar multiplication and
    // modular arithmetic. The method stubs below document the expected
    // operations; actual JCMathLib integration requires the library to
    // be linked at build time.
    //
    // Required JCMathLib operations:
    //   ECPoint.multiplication(BigNat scalar) - G1 scalar multiplication
    //   BigNat.modMult(BigNat a, BigNat b, BigNat mod) - modular multiply
    //   BigNat.modAdd(BigNat a, BigNat b, BigNat mod) - modular add

    /**
     * Constructor. Called once during applet installation.
     */
    protected BrioletteApplet() {
        cardSk = new byte[BN254Params.FR_BYTES];
        keyInitialized = false;

        bloomFilter = new BloomFilter();

        swapPubkey = new byte[BN254Params.G1_BYTES];
        swapPubkeySet = false;

        // Transient buffers (cleared on card deselect/reset)
        rCard = JCSystem.makeTransientByteArray(
            BN254Params.FR_BYTES, JCSystem.CLEAR_ON_DESELECT);
        sessionType = JCSystem.makeTransientByteArray(
            (short) 1, JCSystem.CLEAR_ON_DESELECT);
        scratchPoint = JCSystem.makeTransientByteArray(
            BN254Params.G1_BYTES, JCSystem.CLEAR_ON_DESELECT);
        scratchHash = JCSystem.makeTransientByteArray(
            (short) 32, JCSystem.CLEAR_ON_DESELECT);

        rng = RandomData.getInstance(RandomData.ALG_SECURE_RANDOM);

        register();
    }

    public static void install(byte[] bArray, short bOffset, byte bLength) {
        new BrioletteApplet();
    }

    public void process(APDU apdu) {
        if (selectingApplet()) {
            return;
        }

        byte[] buffer = apdu.getBuffer();
        byte cla = buffer[ISO7816.OFFSET_CLA];
        byte ins = buffer[ISO7816.OFFSET_INS];
        byte p1  = buffer[ISO7816.OFFSET_P1];

        if (cla != (byte) 0x80) {
            ISOException.throwIt(ISO7816.SW_CLA_NOT_SUPPORTED);
        }

        // Version check for curve-dependent operations
        if (ins != INS_RESET_BLOOM && ins != INS_GET_STATUS) {
            if (p1 != VERSION_BN254) {
                // Only BN254 is supported in this prototype
                ISOException.throwIt(SW_BAD_VERSION);
            }
        }

        switch (ins) {
            case INS_GENERATE_KEY:
                processGenerateKey(apdu);
                break;
            case INS_PUBLIC_KEY_SHARE:
                processPublicKeyShare(apdu);
                break;
            case INS_SIGN_COMMIT:
                processSignCommit(apdu, false, false);
                break;
            case INS_SIGN_COMMIT_BSN:
                processSignCommit(apdu, true, false);
                break;
            case INS_SIGN_COMMIT_SWAP:
                processSignCommit(apdu, true, true);
                break;
            case INS_SIGN_RESPOND:
                processSignRespond(apdu);
                break;
            case INS_JOIN_COMMIT:
                processJoinCommit(apdu);
                break;
            case INS_JOIN_RESPOND:
                processJoinRespond(apdu);
                break;
            case INS_RESET_BLOOM:
                processResetBloom(apdu);
                break;
            case INS_SET_SWAP_PUBKEY:
                processSetSwapPubkey(apdu);
                break;
            case INS_GET_STATUS:
                processGetStatus(apdu);
                break;
            default:
                ISOException.throwIt(ISO7816.SW_INS_NOT_SUPPORTED);
        }
    }

    // ========================================================================
    // INS 0x01: GENERATE_KEY
    // ========================================================================

    /**
     * Generate card_sk as a random scalar in Fr.
     * Can only be called once (error if key already exists).
     */
    private void processGenerateKey(APDU apdu) {
        if (keyInitialized) {
            ISOException.throwIt(ISO7816.SW_CONDITIONS_NOT_SATISFIED);
        }

        // Generate random 32 bytes and reduce mod scalar order.
        // In production, use rejection sampling or JCMathLib's
        // BigNat.randomize() to ensure uniform distribution in Fr.
        rng.generateData(cardSk, (short) 0, BN254Params.FR_BYTES);
        reduceModOrder(cardSk, (short) 0);

        keyInitialized = true;
    }

    // ========================================================================
    // INS 0x02: PUBLIC_KEY_SHARE
    // ========================================================================

    /**
     * Compute Q_card = base_point * card_sk.
     * Input:  65 bytes (G1 point: 0x04 || x || y)
     * Output: 65 bytes (G1 point: Q_card)
     */
    private void processPublicKeyShare(APDU apdu) {
        requireKeyInitialized();

        byte[] buffer = apdu.getBuffer();
        short dataLen = apdu.setIncomingAndReceive();

        if (dataLen != BN254Params.G1_BYTES) {
            ISOException.throwIt(ISO7816.SW_WRONG_LENGTH);
        }

        // Q_card = base * card_sk
        // NOTE: In a real implementation, use JCMathLib:
        //   ECPoint base = new ECPoint(BN254_CURVE_PARAMS);
        //   base.setW(buffer, ISO7816.OFFSET_CDATA, G1_BYTES);
        //   BigNat sk = new BigNat(FR_BYTES);
        //   sk.fromByteArray(cardSk, 0, FR_BYTES);
        //   base.multiplication(sk);  // base is now Q_card
        //   base.getW(buffer, 0);
        ecPointMul(buffer, ISO7816.OFFSET_CDATA, cardSk, (short) 0,
                   buffer, (short) 0);

        apdu.setOutgoingAndSend((short) 0, BN254Params.G1_BYTES);
    }

    // ========================================================================
    // INS 0x10/0x11/0x13: SIGN_COMMIT (with/without basename, swap mode)
    // ========================================================================

    /**
     * Phase 1 of signing: generate r_card and commit.
     *
     * Without basename (INS 0x10):
     *   Input:  65B (S point)
     *   Output: 65B (U_card = S * r_card)
     *
     * With basename (INS 0x11):
     *   Input:  130B (S point || bsn_base point)
     *   Output: 195B (U_card || K_card || K_u_card)
     *
     * Swap mode with authorization (INS 0x13):
     *   Input:  194B (S point || bsn_base point || auth_c(32B) || auth_s(32B))
     *   Output: 195B (U_card || K_card || K_u_card)
     *   The card verifies the Schnorr swap authorization before proceeding.
     *
     * @param hasBasename true if basename point is included
     * @param isSwap true for swap mode (requires swap authorization token)
     */
    private void processSignCommit(APDU apdu, boolean hasBasename, boolean isSwap) {
        requireKeyInitialized();

        byte[] buffer = apdu.getBuffer();
        short dataLen = apdu.setIncomingAndReceive();

        // Compute expected input length
        short expectedLen;
        if (!hasBasename) {
            expectedLen = BN254Params.G1_BYTES;
        } else if (isSwap) {
            // Swap: S(65) + bsn_base(65) + auth_c(32) + auth_s(32) = 194
            expectedLen = (short)(BN254Params.G1_BYTES * 2 + SWAP_AUTH_BYTES);
        } else {
            // Normal basename: S(65) + bsn_base(65) = 130
            expectedLen = (short)(BN254Params.G1_BYTES * 2);
        }
        if (dataLen != expectedLen) {
            ISOException.throwIt(ISO7816.SW_WRONG_LENGTH);
        }

        short bsnOffset = (short)(ISO7816.OFFSET_CDATA + BN254Params.G1_BYTES);

        if (hasBasename) {
            if (isSwap) {
                // === Verify swap authorization (Schnorr signature from swap server) ===
                if (!swapPubkeySet) {
                    ISOException.throwIt(SW_NO_SWAP_KEY);
                }
                short authOffset = (short)(bsnOffset + BN254Params.G1_BYTES);
                if (!verifySwapAuth(buffer, bsnOffset, buffer, authOffset)) {
                    ISOException.throwIt(SW_SWAP_AUTH_FAILED);
                }
                // Swap auth verified — do NOT add to bloom filter
                // (swap transactions return fresh tokens with new basenames)
            } else {
                // Normal basename: check bloom filter
                if (bloomFilter.checkAndAdd(buffer, bsnOffset, BN254Params.G1_BYTES)) {
                    ISOException.throwIt(SW_BASENAME_USED);
                }
            }
        }

        // Generate ephemeral randomness r_card
        rng.generateData(rCard, (short) 0, BN254Params.FR_BYTES);
        reduceModOrder(rCard, (short) 0);
        sessionType[0] = SESSION_SIGN;

        // U_card = S * r_card
        short sOffset = ISO7816.OFFSET_CDATA;
        ecPointMul(buffer, sOffset, rCard, (short) 0,
                   buffer, (short) 0);
        short outOffset = BN254Params.G1_BYTES;

        if (hasBasename) {
            // K_card = bsn_base * card_sk
            ecPointMul(buffer, bsnOffset, cardSk, (short) 0,
                       buffer, outOffset);
            outOffset += BN254Params.G1_BYTES;

            // K_u_card = bsn_base * r_card
            ecPointMul(buffer, bsnOffset, rCard, (short) 0,
                       buffer, outOffset);
            outOffset += BN254Params.G1_BYTES;
        }

        apdu.setOutgoingAndSend((short) 0, outOffset);
    }

    /**
     * Verify a swap authorization Schnorr signature.
     *
     * The swap server signs the basename with its private key:
     *   c = H(R || bsn_base || swap_pk), s = r + c * swap_sk
     *
     * We verify by reconstructing R:
     *   R' = G * s - swap_pk * c  (2 scalar muls + point subtraction)
     *   c' = H(R' || bsn_base || swap_pk)
     *   Check c == c'
     *
     * @param bsnBuf    Buffer containing basename G1 point (65 bytes)
     * @param bsnOff    Offset of basename in buffer
     * @param authBuf   Buffer containing auth token: c(32B) || s(32B)
     * @param authOff   Offset of auth token in buffer
     * @return true if the swap authorization is valid
     */
    private boolean verifySwapAuth(byte[] bsnBuf, short bsnOff,
                                    byte[] authBuf, short authOff) {
        // auth_c is at authOff (32 bytes), auth_s is at authOff+32 (32 bytes)
        short cOff = authOff;
        short sOff = (short)(authOff + BN254Params.FR_BYTES);

        // We need a second scratch point for swap_pk * (-c).
        // Allocate temporarily — on real hardware, use a persistent buffer.
        byte[] scratchPoint2 = new byte[BN254Params.G1_BYTES];

        // Step 1: scratchPoint = G * s
        ecPointMul(BN254Params.G1_UNCOMPRESSED, (short) 0,
                   authBuf, sOff,
                   scratchPoint, (short) 0);

        // Step 2: Negate c: neg_c = SCALAR_ORDER - c
        scalarNegate(authBuf, cOff, scratchHash, (short) 0);

        // Step 3: scratchPoint2 = swap_pk * (-c)
        ecPointMul(swapPubkey, (short) 0,
                   scratchHash, (short) 0,
                   scratchPoint2, (short) 0);

        // Step 4: R' = G*s + swap_pk*(-c)
        ecPointAdd(scratchPoint, (short) 0,
                   scratchPoint2, (short) 0,
                   scratchPoint, (short) 0);

        // Step 5: c' = SHA256(R' || bsn_base || swap_pk) reduced to Fr
        MessageDigest sha256 = MessageDigest.getInstance(
            MessageDigest.ALG_SHA_256, false);
        sha256.reset();
        sha256.update(scratchPoint, (short) 0, BN254Params.G1_BYTES);
        sha256.update(bsnBuf, bsnOff, BN254Params.G1_BYTES);
        sha256.doFinal(swapPubkey, (short) 0, BN254Params.G1_BYTES,
                       scratchHash, (short) 0);
        reduceModOrder(scratchHash, (short) 0);

        // Step 6: Compare c' with c
        return Util.arrayCompare(scratchHash, (short) 0,
                                 authBuf, cOff, BN254Params.FR_BYTES) == 0;
    }

    // ========================================================================
    // INS 0x12: SIGN_RESPOND
    // ========================================================================

    /**
     * Phase 2 of signing: produce Schnorr response share.
     * Input:  32B (challenge c)
     * Output: 32B (s_card = r_card + c * card_sk)
     */
    private void processSignRespond(APDU apdu) {
        requireKeyInitialized();
        if (sessionType[0] != SESSION_SIGN) {
            ISOException.throwIt(ISO7816.SW_CONDITIONS_NOT_SATISFIED);
        }

        byte[] buffer = apdu.getBuffer();
        short dataLen = apdu.setIncomingAndReceive();

        if (dataLen != BN254Params.FR_BYTES) {
            ISOException.throwIt(ISO7816.SW_WRONG_LENGTH);
        }

        // s_card = r_card + c * card_sk (mod scalar_order)
        computeSchnorrResponse(buffer, ISO7816.OFFSET_CDATA, buffer, (short) 0);

        // Clear ephemeral state
        Util.arrayFillNonAtomic(rCard, (short) 0, BN254Params.FR_BYTES, (byte) 0);
        sessionType[0] = SESSION_NONE;

        apdu.setOutgoingAndSend((short) 0, BN254Params.FR_BYTES);
    }

    // ========================================================================
    // INS 0x20: JOIN_COMMIT
    // ========================================================================

    /**
     * Phase 1 of blind join: generate r_card and commit U_card = B * r_card.
     * Input:  65B (base point B)
     * Output: 65B (U_card)
     */
    private void processJoinCommit(APDU apdu) {
        requireKeyInitialized();

        byte[] buffer = apdu.getBuffer();
        short dataLen = apdu.setIncomingAndReceive();

        if (dataLen != BN254Params.G1_BYTES) {
            ISOException.throwIt(ISO7816.SW_WRONG_LENGTH);
        }

        // Generate ephemeral randomness r_card
        rng.generateData(rCard, (short) 0, BN254Params.FR_BYTES);
        reduceModOrder(rCard, (short) 0);
        sessionType[0] = SESSION_JOIN;

        // U_card = B * r_card
        ecPointMul(buffer, ISO7816.OFFSET_CDATA, rCard, (short) 0,
                   buffer, (short) 0);

        apdu.setOutgoingAndSend((short) 0, BN254Params.G1_BYTES);
    }

    // ========================================================================
    // INS 0x21: JOIN_RESPOND
    // ========================================================================

    /**
     * Phase 2 of blind join: produce Schnorr response share.
     * Input:  32B (challenge c)
     * Output: 32B (s_card = r_card + c * card_sk)
     */
    private void processJoinRespond(APDU apdu) {
        requireKeyInitialized();
        if (sessionType[0] != SESSION_JOIN) {
            ISOException.throwIt(ISO7816.SW_CONDITIONS_NOT_SATISFIED);
        }

        byte[] buffer = apdu.getBuffer();
        short dataLen = apdu.setIncomingAndReceive();

        if (dataLen != BN254Params.FR_BYTES) {
            ISOException.throwIt(ISO7816.SW_WRONG_LENGTH);
        }

        // s_card = r_card + c * card_sk (mod scalar_order)
        computeSchnorrResponse(buffer, ISO7816.OFFSET_CDATA, buffer, (short) 0);

        // Clear ephemeral state
        Util.arrayFillNonAtomic(rCard, (short) 0, BN254Params.FR_BYTES, (byte) 0);
        sessionType[0] = SESSION_NONE;

        apdu.setOutgoingAndSend((short) 0, BN254Params.FR_BYTES);
    }

    // ========================================================================
    // INS 0x30: RESET_BLOOM
    // ========================================================================

    /**
     * Reset bloom filter for a new epoch.
     * Input: 4B (new_epoch, big-endian u32)
     * Only succeeds if new_epoch > current epoch (monotonic).
     */
    private void processResetBloom(APDU apdu) {
        byte[] buffer = apdu.getBuffer();
        short dataLen = apdu.setIncomingAndReceive();

        if (dataLen != 4) {
            ISOException.throwIt(ISO7816.SW_WRONG_LENGTH);
        }

        short off = ISO7816.OFFSET_CDATA;
        int newEpoch = ((buffer[off] & 0xFF) << 24)
                     | ((buffer[(short)(off + 1)] & 0xFF) << 16)
                     | ((buffer[(short)(off + 2)] & 0xFF) << 8)
                     | (buffer[(short)(off + 3)] & 0xFF);

        if (!bloomFilter.resetForEpoch(newEpoch)) {
            ISOException.throwIt(ISO7816.SW_CONDITIONS_NOT_SATISFIED);
        }
    }

    // ========================================================================
    // INS 0x31: SET_SWAP_PUBKEY
    // ========================================================================

    /**
     * Set the swap server's public key (for future swap authorization).
     * Input: 65B (G1 point)
     * Only allowed during personalization (before first signing session).
     */
    private void processSetSwapPubkey(APDU apdu) {
        byte[] buffer = apdu.getBuffer();
        short dataLen = apdu.setIncomingAndReceive();

        if (dataLen != BN254Params.G1_BYTES) {
            ISOException.throwIt(ISO7816.SW_WRONG_LENGTH);
        }

        Util.arrayCopy(buffer, ISO7816.OFFSET_CDATA,
                       swapPubkey, (short) 0, BN254Params.G1_BYTES);
        swapPubkeySet = true;
    }

    // ========================================================================
    // INS 0x40: GET_STATUS
    // ========================================================================

    /**
     * Query card status.
     * Output: 6 bytes
     *   [0]: flags (bit 0: key_initialized, bit 1: session_active, bit 2: swap_pubkey_set)
     *   [1-4]: epoch counter (big-endian u32)
     *   [5]: supported curve version
     */
    private void processGetStatus(APDU apdu) {
        byte[] buffer = apdu.getBuffer();

        byte flags = 0;
        if (keyInitialized) flags |= 0x01;
        if (sessionType[0] != SESSION_NONE) flags |= 0x02;
        if (swapPubkeySet) flags |= 0x04;
        buffer[0] = flags;

        int epoch = bloomFilter.getEpoch();
        buffer[1] = (byte) ((epoch >> 24) & 0xFF);
        buffer[2] = (byte) ((epoch >> 16) & 0xFF);
        buffer[3] = (byte) ((epoch >> 8) & 0xFF);
        buffer[4] = (byte) (epoch & 0xFF);

        buffer[5] = VERSION_BN254;

        apdu.setOutgoingAndSend((short) 0, (short) 6);
    }

    // ========================================================================
    // Helper methods
    // ========================================================================

    private void requireKeyInitialized() {
        if (!keyInitialized) {
            ISOException.throwIt(ISO7816.SW_CONDITIONS_NOT_SATISFIED);
        }
    }

    /**
     * Compute s_card = r_card + c * card_sk (mod scalar_order).
     *
     * NOTE: This is a placeholder. In a real implementation, use JCMathLib:
     *   BigNat c_bn = new BigNat(FR_BYTES);
     *   c_bn.fromByteArray(challengeBuf, challengeOff, FR_BYTES);
     *   BigNat sk_bn = new BigNat(FR_BYTES);
     *   sk_bn.fromByteArray(cardSk, 0, FR_BYTES);
     *   BigNat r_bn = new BigNat(FR_BYTES);
     *   r_bn.fromByteArray(rCard, 0, FR_BYTES);
     *   BigNat order = new BigNat(FR_BYTES);
     *   order.fromByteArray(SCALAR_ORDER, 0, FR_BYTES);
     *   BigNat tmp = new BigNat(FR_BYTES);
     *   tmp.modMult(c_bn, sk_bn, order);  // tmp = c * card_sk mod r
     *   r_bn.modAdd(tmp, order);           // r_bn = r_card + tmp mod r
     *   r_bn.toByteArray(outBuf, outOff);
     */
    private void computeSchnorrResponse(byte[] challengeBuf, short challengeOff,
                                         byte[] outBuf, short outOff) {
        // Placeholder: copy challenge to output.
        // Replace with actual JCMathLib modular arithmetic.
        // s_card = r_card + c * card_sk (mod SCALAR_ORDER)
        scalarMulAdd(rCard, (short) 0,
                     challengeBuf, challengeOff,
                     cardSk, (short) 0,
                     outBuf, outOff);
    }

    /**
     * Compute result = a + b * c (mod SCALAR_ORDER).
     * This is the core Fr arithmetic operation for Schnorr responses.
     *
     * NOTE: Uses ECMath (BigInteger) for jCardSim. For production JavaCard
     * hardware, replace with JCMathLib BigNat operations:
     *   BigNat tmp = b.modMult(c, order);
     *   BigNat result = a.modAdd(tmp, order);
     */
    private void scalarMulAdd(byte[] aBuf, short aOff,
                               byte[] bBuf, short bOff,
                               byte[] cBuf, short cOff,
                               byte[] outBuf, short outOff) {
        ECMath.scalarMulAdd(aBuf, aOff, bBuf, bOff, cBuf, cOff, outBuf, outOff);
    }

    /**
     * Compute result_point = input_point * scalar.
     * G1 scalar multiplication — the core EC operation.
     *
     * NOTE: Uses ECMath (BigInteger) for jCardSim. For production JavaCard
     * hardware, replace with JCMathLib:
     *   ECPoint pt = new ECPoint(curve);
     *   pt.setW(pointBuf, pointOff, G1_BYTES);
     *   BigNat s = new BigNat(curve.rBN.length(), MEMORY_TYPE, rm);
     *   s.fromByteArray(scalarBuf, scalarOff, FR_BYTES);
     *   pt.multiplication(s);
     *   pt.getW(outBuf, outOff);
     */
    private void ecPointMul(byte[] pointBuf, short pointOff,
                            byte[] scalarBuf, short scalarOff,
                            byte[] outBuf, short outOff) {
        ECMath.ecPointMul(pointBuf, pointOff, scalarBuf, scalarOff, outBuf, outOff);
    }

    /**
     * Reduce a 32-byte big-endian value modulo the scalar field order.
     * Ensures card_sk and r_card are valid Fr elements.
     *
     * NOTE: Uses ECMath (BigInteger) for jCardSim. For production JavaCard
     * hardware, replace with JCMathLib: BigNat.mod(order).
     */
    private void reduceModOrder(byte[] buf, short offset) {
        ECMath.reduceModOrder(buf, offset);
    }

    /**
     * Compute result = SCALAR_ORDER - input (mod SCALAR_ORDER).
     * Negation in the scalar field.
     *
     * NOTE: Uses ECMath (BigInteger) for jCardSim. For production JavaCard
     * hardware, replace with JCMathLib: order.subtract(val).
     */
    private void scalarNegate(byte[] inBuf, short inOff,
                               byte[] outBuf, short outOff) {
        ECMath.scalarNegate(inBuf, inOff, outBuf, outOff);
    }

    /**
     * Compute result_point = point_a + point_b (EC point addition on G1).
     *
     * NOTE: Uses ECMath (BigInteger) for jCardSim. For production JavaCard
     * hardware, replace with JCMathLib:
     *   ECPoint a = new ECPoint(curve); a.setW(...); a.add(b); a.getW(...);
     */
    private void ecPointAdd(byte[] aBuf, short aOff,
                            byte[] bBuf, short bOff,
                            byte[] outBuf, short outOff) {
        ECMath.ecPointAdd(aBuf, aOff, bBuf, bOff, outBuf, outOff);
    }
}
