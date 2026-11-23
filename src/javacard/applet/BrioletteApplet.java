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
import javacard.security.ECPublicKey;
import javacard.security.ECPrivateKey;
import javacard.security.KeyBuilder;
import javacard.security.KeyPair;
import javacard.security.MessageDigest;
import javacard.security.RandomData;
import javacard.security.Signature;

/**
 * Briolette ECDAA split-key signing applet for JavaCard.
 *
 * Implements the card side of the Brickell & Li style split-key ECDAA protocol.
 * The card performs only G1 scalar multiplications and Fr scalar arithmetic —
 * no pairings, no G2/GT operations.
 *
 * Supports two curves, selected at GENERATE_KEY time and locked for the card's
 * lifetime:
 *   - BN254 (v0): uncompressed G1 points (65 bytes), ~100-bit security
 *   - BLS12-381 (v1): compressed G1 points (48 bytes), 128-bit security
 *
 * The scalar field (Fr) is 32 bytes for both curves.
 *
 * APDU Protocol:
 *   CLA: 0x80
 *   P1:  Curve version (0x00 = BN254/v0, 0x01 = BLS12-381/v1)
 *   P2:  Reserved (0x00)
 *
 *   INS 0x01: GENERATE_KEY       - Generate card_sk (locks curve version)
 *   INS 0x02: PUBLIC_KEY_SHARE   - Compute Q_card = base * card_sk
 *   INS 0x10: SIGN_COMMIT        - Phase 1 signing (no basename)
 *   INS 0x11: SIGN_COMMIT_BSN    - Phase 1 signing (with basename, bloom check)
 *   INS 0x12: SIGN_RESPOND       - Phase 2 signing
 *   INS 0x13: SIGN_COMMIT_BSN_SWAP - Phase 1 signing (swap mode, skip bloom)
 *   INS 0x20: JOIN_COMMIT        - Phase 1 blind join
 *   INS 0x21: JOIN_RESPOND       - Phase 2 blind join
 *   INS 0x30: RESET_BLOOM        - Reset bloom filter for new epoch (auth required if key set)
 *   INS 0x31: SET_SWAP_PUBKEY    - Set swap server public key
 *   INS 0x32: SET_RESET_PUBKEY   - Set epoch reset authorization public key
 *   INS 0x40: GET_STATUS         - Query card status
 *
 *   Manufacturer attestation (P-256 ECDSA, curve-independent):
 *   INS 0x60: MFR_GENERATE_KEY   - Generate P-256 attestation keypair
 *   INS 0x61: MFR_SET_CERT       - Load manufacturer certificate (CA sig over card pk)
 *   INS 0x62: MFR_ATTEST         - Sign challenge, return sig + cert + pubkey
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
    private static final byte INS_SET_RESET_PUBKEY   = (byte) 0x32;
    private static final byte INS_GET_STATUS         = (byte) 0x40;

    // Manufacturer attestation (P-256 ECDSA)
    private static final byte INS_MFR_GENERATE_KEY  = (byte) 0x60;
    private static final byte INS_MFR_SET_CERT      = (byte) 0x61;
    private static final byte INS_MFR_ATTEST        = (byte) 0x62;

    // ========================================================================
    // Curve version constants
    // ========================================================================
    private static final byte VERSION_BN254   = (byte) 0x00;
    private static final byte VERSION_BLS381  = (byte) 0x01;

    // ========================================================================
    // Status words
    // ========================================================================
    private static final short SW_BASENAME_USED      = (short) 0x6A84;
    private static final short SW_BAD_VERSION        = (short) 0x6A86;
    private static final short SW_SWAP_AUTH_FAILED   = (short) 0x6A85;
    private static final short SW_NO_SWAP_KEY        = (short) 0x6A87;
    private static final short SW_RESET_AUTH_FAILED  = (short) 0x6A88;
    private static final short SW_NO_RESET_KEY       = (short) 0x6A89;
    private static final short SW_PERSONALIZED       = (short) 0x6A8A;
    private static final short SW_MFR_NOT_READY      = (short) 0x6A8B;
    private static final short SW_MFR_NO_CERT        = (short) 0x6A8C;

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

    /** Swap server's public key (G1 point in wire format). */
    private byte[] swapPubkey;

    /** Whether swap pubkey has been set. */
    private boolean swapPubkeySet;

    /** Epoch reset authorization public key (G1 point in wire format). */
    private byte[] resetPubkey;

    /** Whether reset pubkey has been set. */
    private boolean resetPubkeySet;

    /** Set on first signing/join operation; locks pubkey configuration. */
    private boolean personalized;

    // ---- Manufacturer attestation (P-256 ECDSA) ----

    /** P-256 keypair for manufacturer attestation. */
    private KeyPair mfrKeyPair;

    /** Whether the manufacturer P-256 key has been generated. */
    private boolean mfrKeyGenerated;

    /**
     * Manufacturer certificate: the manufacturer's ECDSA-SHA256 signature
     * over this card's P-256 public key (DER-encoded, max ~72 bytes).
     */
    private byte[] mfrCert;

    /** Actual length of the stored manufacturer certificate. */
    private short mfrCertLen;

    /** Whether a manufacturer certificate has been loaded. */
    private boolean mfrCertSet;

    /** ECDSA signature instance for attestation signing. */
    private Signature mfrSigner;

    /** Locked curve version (set at GENERATE_KEY time). */
    private byte activeCurveVersion;

    /** Active G1 point size (65 for BN254, 48 for BLS12-381). */
    private short activeG1Bytes;

    /** Active Fr scalar size (32 for both curves). */
    private short activeFrBytes;

    /** Generator point for the active curve (in wire format). */
    private byte[] activeGenerator;

    // ========================================================================
    // Transient state (RAM, cleared on deselect)
    // ========================================================================

    private byte[] rCard;
    private byte[] sessionType;
    private byte[] scratchPoint;
    private byte[] scratchHash;

    // ========================================================================
    // Crypto instances
    // ========================================================================
    private RandomData rng;

    /**
     * Constructor. Allocates with maximum sizes; actual sizes are set at
     * GENERATE_KEY time when the curve is selected.
     */
    protected BrioletteApplet() {
        // Fr is 32 bytes for both curves
        cardSk = new byte[32];
        keyInitialized = false;
        activeCurveVersion = (byte) 0xFF; // unset

        bloomFilter = new BloomFilter();

        // Allocate with max G1 size (BN254 uncompressed = 65 bytes)
        // BLS12-381 compressed = 48 bytes, so 65 is sufficient for both
        swapPubkey = new byte[BN254Params.G1_BYTES];
        swapPubkeySet = false;
        resetPubkey = new byte[BN254Params.G1_BYTES];
        resetPubkeySet = false;
        personalized = false;

        // Transient buffers (cleared on card deselect/reset)
        rCard = JCSystem.makeTransientByteArray((short) 32, JCSystem.CLEAR_ON_DESELECT);
        sessionType = JCSystem.makeTransientByteArray((short) 1, JCSystem.CLEAR_ON_DESELECT);
        scratchPoint = JCSystem.makeTransientByteArray(
            BN254Params.G1_BYTES, JCSystem.CLEAR_ON_DESELECT);
        scratchHash = JCSystem.makeTransientByteArray(
            (short) 32, JCSystem.CLEAR_ON_DESELECT);

        rng = RandomData.getInstance(RandomData.ALG_SECURE_RANDOM);

        // Manufacturer attestation: P-256 ECDSA
        mfrKeyPair = new KeyPair(KeyPair.ALG_EC_FP, KeyBuilder.LENGTH_EC_FP_256);
        mfrKeyGenerated = false;
        mfrCert = new byte[72]; // Max DER-encoded ECDSA-SHA256 signature
        mfrCertLen = 0;
        mfrCertSet = false;
        mfrSigner = Signature.getInstance(Signature.ALG_ECDSA_SHA_256, false);

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

        // Version check for curve-dependent operations.
        // Manufacturer attestation APDUs (0x60-0x62) use P-256 natively
        // and don't depend on the BN254/BLS381 curve selection.
        if (ins != INS_RESET_BLOOM && ins != INS_GET_STATUS
                && ins != INS_MFR_GENERATE_KEY && ins != INS_MFR_SET_CERT
                && ins != INS_MFR_ATTEST) {
            if (ins == INS_GENERATE_KEY) {
                // GENERATE_KEY accepts BN254 or BLS381 to select the curve
                if (p1 != VERSION_BN254 && p1 != VERSION_BLS381) {
                    ISOException.throwIt(SW_BAD_VERSION);
                }
            } else {
                // All other commands must match the locked curve version
                if (!keyInitialized) {
                    // Key not generated yet — reject since we need an active curve
                    ISOException.throwIt(ISO7816.SW_CONDITIONS_NOT_SATISFIED);
                }
                if (p1 != activeCurveVersion) {
                    ISOException.throwIt(SW_BAD_VERSION);
                }
            }
        }

        switch (ins) {
            case INS_GENERATE_KEY:
                processGenerateKey(apdu, p1);
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
            case INS_SET_RESET_PUBKEY:
                processSetResetPubkey(apdu);
                break;
            case INS_GET_STATUS:
                processGetStatus(apdu);
                break;
            case INS_MFR_GENERATE_KEY:
                processMfrGenerateKey(apdu);
                break;
            case INS_MFR_SET_CERT:
                processMfrSetCert(apdu);
                break;
            case INS_MFR_ATTEST:
                processMfrAttest(apdu);
                break;
            default:
                ISOException.throwIt(ISO7816.SW_INS_NOT_SUPPORTED);
        }
    }

    // ========================================================================
    // INS 0x01: GENERATE_KEY (locks curve version)
    // ========================================================================

    private void processGenerateKey(APDU apdu, byte curveVersion) {
        if (keyInitialized) {
            ISOException.throwIt(ISO7816.SW_CONDITIONS_NOT_SATISFIED);
        }

        // Lock the curve version and initialize ECMath
        activeCurveVersion = curveVersion;
        ECMath.initCurve(curveVersion);

        if (curveVersion == VERSION_BN254) {
            activeG1Bytes = BN254Params.G1_BYTES;
            activeFrBytes = BN254Params.FR_BYTES;
            activeGenerator = BN254Params.G1_UNCOMPRESSED;
        } else {
            activeG1Bytes = BLS12381Params.G1_BYTES;
            activeFrBytes = BLS12381Params.FR_BYTES;
            activeGenerator = BLS12381Params.G1_COMPRESSED;
        }

        // Generate random scalar and reduce mod order
        rng.generateData(cardSk, (short) 0, activeFrBytes);
        reduceModOrder(cardSk, (short) 0);

        keyInitialized = true;
    }

    // ========================================================================
    // INS 0x02: PUBLIC_KEY_SHARE
    // ========================================================================

    private void processPublicKeyShare(APDU apdu) {
        requireKeyInitialized();

        byte[] buffer = apdu.getBuffer();
        short dataLen = apdu.setIncomingAndReceive();

        if (dataLen != activeG1Bytes) {
            ISOException.throwIt(ISO7816.SW_WRONG_LENGTH);
        }

        ecPointMul(buffer, ISO7816.OFFSET_CDATA, cardSk, (short) 0,
                   buffer, (short) 0);

        apdu.setOutgoingAndSend((short) 0, activeG1Bytes);
    }

    // ========================================================================
    // INS 0x10/0x11/0x13: SIGN_COMMIT
    // ========================================================================

    private void processSignCommit(APDU apdu, boolean hasBasename, boolean isSwap) {
        requireKeyInitialized();
        personalized = true;

        byte[] buffer = apdu.getBuffer();
        short dataLen = apdu.setIncomingAndReceive();

        short expectedLen;
        if (!hasBasename) {
            expectedLen = activeG1Bytes;
        } else if (isSwap) {
            expectedLen = (short)(activeG1Bytes * 2 + SWAP_AUTH_BYTES);
        } else {
            expectedLen = (short)(activeG1Bytes * 2);
        }
        if (dataLen != expectedLen) {
            ISOException.throwIt(ISO7816.SW_WRONG_LENGTH);
        }

        short bsnOffset = (short)(ISO7816.OFFSET_CDATA + activeG1Bytes);

        if (hasBasename) {
            if (isSwap) {
                if (!swapPubkeySet) {
                    ISOException.throwIt(SW_NO_SWAP_KEY);
                }
                short authOffset = (short)(bsnOffset + activeG1Bytes);
                if (!verifySwapAuth(buffer, bsnOffset, buffer, authOffset)) {
                    ISOException.throwIt(SW_SWAP_AUTH_FAILED);
                }
            } else {
                if (bloomFilter.checkAndAdd(buffer, bsnOffset, activeG1Bytes)) {
                    ISOException.throwIt(SW_BASENAME_USED);
                }
            }
        }

        // Generate ephemeral randomness
        rng.generateData(rCard, (short) 0, activeFrBytes);
        reduceModOrder(rCard, (short) 0);
        sessionType[0] = SESSION_SIGN;

        // U_card = S * r_card
        short sOffset = ISO7816.OFFSET_CDATA;
        ecPointMul(buffer, sOffset, rCard, (short) 0,
                   buffer, (short) 0);
        short outOffset = activeG1Bytes;

        if (hasBasename) {
            // K_card = bsn_base * card_sk
            ecPointMul(buffer, bsnOffset, cardSk, (short) 0,
                       buffer, outOffset);
            outOffset += activeG1Bytes;

            // K_u_card = bsn_base * r_card
            ecPointMul(buffer, bsnOffset, rCard, (short) 0,
                       buffer, outOffset);
            outOffset += activeG1Bytes;
        }

        apdu.setOutgoingAndSend((short) 0, outOffset);
    }

    /**
     * Verify a Schnorr authorization signature against a given public key.
     * Used by both swap authorization and epoch reset authorization.
     *
     * @param pubkey     The verifying public key (G1 point).
     * @param ctxBuf     Buffer containing the context data to bind to.
     * @param ctxOff     Offset of context data in ctxBuf.
     * @param ctxLen     Length of context data.
     * @param authBuf    Buffer containing the Schnorr signature (c || s).
     * @param authOff    Offset of the signature in authBuf.
     */
    private boolean verifySchnorrAuth(byte[] pubkey,
                                       byte[] ctxBuf, short ctxOff, short ctxLen,
                                       byte[] authBuf, short authOff) {
        short cOff = authOff;
        short sOff = (short)(authOff + activeFrBytes);

        byte[] scratchPoint2 = new byte[activeG1Bytes];

        // Step 1: scratchPoint = G * s
        ecPointMul(activeGenerator, (short) 0,
                   authBuf, sOff,
                   scratchPoint, (short) 0);

        // Step 2: Negate c
        scalarNegate(authBuf, cOff, scratchHash, (short) 0);

        // Step 3: scratchPoint2 = pubkey * (-c)
        ecPointMul(pubkey, (short) 0,
                   scratchHash, (short) 0,
                   scratchPoint2, (short) 0);

        // Step 4: R' = G*s + pubkey*(-c)
        ecPointAdd(scratchPoint, (short) 0,
                   scratchPoint2, (short) 0,
                   scratchPoint, (short) 0);

        // Step 5: c' = SHA256(R' || context || pubkey) reduced to Fr
        MessageDigest sha256 = MessageDigest.getInstance(
            MessageDigest.ALG_SHA_256, false);
        sha256.reset();
        sha256.update(scratchPoint, (short) 0, activeG1Bytes);
        sha256.update(ctxBuf, ctxOff, ctxLen);
        sha256.doFinal(pubkey, (short) 0, activeG1Bytes,
                       scratchHash, (short) 0);
        reduceModOrder(scratchHash, (short) 0);

        // Step 6: Compare
        return Util.arrayCompare(scratchHash, (short) 0,
                                 authBuf, cOff, activeFrBytes) == 0;
    }

    /**
     * Verify a swap authorization Schnorr signature (legacy wrapper).
     */
    private boolean verifySwapAuth(byte[] bsnBuf, short bsnOff,
                                    byte[] authBuf, short authOff) {
        return verifySchnorrAuth(swapPubkey,
                                 bsnBuf, bsnOff, activeG1Bytes,
                                 authBuf, authOff);
    }

    // ========================================================================
    // INS 0x12: SIGN_RESPOND
    // ========================================================================

    private void processSignRespond(APDU apdu) {
        requireKeyInitialized();
        if (sessionType[0] != SESSION_SIGN) {
            ISOException.throwIt(ISO7816.SW_CONDITIONS_NOT_SATISFIED);
        }

        byte[] buffer = apdu.getBuffer();
        short dataLen = apdu.setIncomingAndReceive();

        if (dataLen != activeFrBytes) {
            ISOException.throwIt(ISO7816.SW_WRONG_LENGTH);
        }

        computeSchnorrResponse(buffer, ISO7816.OFFSET_CDATA, buffer, (short) 0);

        Util.arrayFillNonAtomic(rCard, (short) 0, activeFrBytes, (byte) 0);
        sessionType[0] = SESSION_NONE;

        apdu.setOutgoingAndSend((short) 0, activeFrBytes);
    }

    // ========================================================================
    // INS 0x20: JOIN_COMMIT
    // ========================================================================

    private void processJoinCommit(APDU apdu) {
        requireKeyInitialized();
        personalized = true;

        byte[] buffer = apdu.getBuffer();
        short dataLen = apdu.setIncomingAndReceive();

        if (dataLen != activeG1Bytes) {
            ISOException.throwIt(ISO7816.SW_WRONG_LENGTH);
        }

        rng.generateData(rCard, (short) 0, activeFrBytes);
        reduceModOrder(rCard, (short) 0);
        sessionType[0] = SESSION_JOIN;

        ecPointMul(buffer, ISO7816.OFFSET_CDATA, rCard, (short) 0,
                   buffer, (short) 0);

        apdu.setOutgoingAndSend((short) 0, activeG1Bytes);
    }

    // ========================================================================
    // INS 0x21: JOIN_RESPOND
    // ========================================================================

    private void processJoinRespond(APDU apdu) {
        requireKeyInitialized();
        if (sessionType[0] != SESSION_JOIN) {
            ISOException.throwIt(ISO7816.SW_CONDITIONS_NOT_SATISFIED);
        }

        byte[] buffer = apdu.getBuffer();
        short dataLen = apdu.setIncomingAndReceive();

        if (dataLen != activeFrBytes) {
            ISOException.throwIt(ISO7816.SW_WRONG_LENGTH);
        }

        computeSchnorrResponse(buffer, ISO7816.OFFSET_CDATA, buffer, (short) 0);

        Util.arrayFillNonAtomic(rCard, (short) 0, activeFrBytes, (byte) 0);
        sessionType[0] = SESSION_NONE;

        apdu.setOutgoingAndSend((short) 0, activeFrBytes);
    }

    // ========================================================================
    // INS 0x30: RESET_BLOOM
    // ========================================================================

    private void processResetBloom(APDU apdu) {
        byte[] buffer = apdu.getBuffer();
        short dataLen = apdu.setIncomingAndReceive();

        short epochBytes = 4;
        if (resetPubkeySet) {
            // When a reset public key is set, the APDU must include:
            // 4 bytes epoch + 64 bytes Schnorr signature (c || s)
            if (dataLen != (short)(epochBytes + SWAP_AUTH_BYTES)) {
                ISOException.throwIt(ISO7816.SW_WRONG_LENGTH);
            }
        } else {
            if (dataLen != epochBytes) {
                ISOException.throwIt(ISO7816.SW_WRONG_LENGTH);
            }
        }

        short off = ISO7816.OFFSET_CDATA;
        int newEpoch = ((buffer[off] & 0xFF) << 24)
                     | ((buffer[(short)(off + 1)] & 0xFF) << 16)
                     | ((buffer[(short)(off + 2)] & 0xFF) << 8)
                     | (buffer[(short)(off + 3)] & 0xFF);

        if (resetPubkeySet) {
            // Verify the Schnorr signature over the epoch bytes.
            short authOff = (short)(off + epochBytes);
            if (!verifySchnorrAuth(resetPubkey,
                                    buffer, off, epochBytes,
                                    buffer, authOff)) {
                ISOException.throwIt(SW_RESET_AUTH_FAILED);
            }
        }

        if (!bloomFilter.resetForEpoch(newEpoch)) {
            ISOException.throwIt(ISO7816.SW_CONDITIONS_NOT_SATISFIED);
        }
    }

    // ========================================================================
    // INS 0x31: SET_SWAP_PUBKEY
    // ========================================================================

    private void processSetSwapPubkey(APDU apdu) {
        requireKeyInitialized();
        if (personalized) {
            ISOException.throwIt(SW_PERSONALIZED);
        }

        byte[] buffer = apdu.getBuffer();
        short dataLen = apdu.setIncomingAndReceive();

        if (dataLen != activeG1Bytes) {
            ISOException.throwIt(ISO7816.SW_WRONG_LENGTH);
        }

        Util.arrayCopy(buffer, ISO7816.OFFSET_CDATA,
                       swapPubkey, (short) 0, activeG1Bytes);
        swapPubkeySet = true;
    }

    // ========================================================================
    // INS 0x32: SET_RESET_PUBKEY
    // ========================================================================

    private void processSetResetPubkey(APDU apdu) {
        requireKeyInitialized();
        if (personalized) {
            ISOException.throwIt(SW_PERSONALIZED);
        }

        byte[] buffer = apdu.getBuffer();
        short dataLen = apdu.setIncomingAndReceive();

        if (dataLen != activeG1Bytes) {
            ISOException.throwIt(ISO7816.SW_WRONG_LENGTH);
        }

        Util.arrayCopy(buffer, ISO7816.OFFSET_CDATA,
                       resetPubkey, (short) 0, activeG1Bytes);
        resetPubkeySet = true;
    }

    // ========================================================================
    // INS 0x40: GET_STATUS
    // ========================================================================

    private void processGetStatus(APDU apdu) {
        byte[] buffer = apdu.getBuffer();

        byte flags = 0;
        if (keyInitialized) flags |= 0x01;
        if (sessionType[0] != SESSION_NONE) flags |= 0x02;
        if (swapPubkeySet) flags |= 0x04;
        if (resetPubkeySet) flags |= 0x08;
        if (personalized) flags |= 0x10;
        if (mfrKeyGenerated) flags |= 0x20;
        if (mfrCertSet) flags |= 0x40;
        buffer[0] = flags;

        int epoch = bloomFilter.getEpoch();
        buffer[1] = (byte) ((epoch >> 24) & 0xFF);
        buffer[2] = (byte) ((epoch >> 16) & 0xFF);
        buffer[3] = (byte) ((epoch >> 8) & 0xFF);
        buffer[4] = (byte) (epoch & 0xFF);

        buffer[5] = keyInitialized ? activeCurveVersion : VERSION_BN254;

        apdu.setOutgoingAndSend((short) 0, (short) 6);
    }

    // ========================================================================
    // INS 0x60: MFR_GENERATE_KEY — Generate P-256 attestation keypair
    // ========================================================================

    /**
     * Generate the card's P-256 manufacturer attestation keypair.
     * Returns the uncompressed public key (65 bytes: 0x04 || x || y).
     * Can only be called once; the key is permanent.
     */
    private void processMfrGenerateKey(APDU apdu) {
        if (mfrKeyGenerated) {
            ISOException.throwIt(ISO7816.SW_CONDITIONS_NOT_SATISFIED);
        }

        mfrKeyPair.genKeyPair();
        mfrKeyGenerated = true;

        // Export the public key (uncompressed P-256 = 65 bytes)
        byte[] buffer = apdu.getBuffer();
        ECPublicKey pub = (ECPublicKey) mfrKeyPair.getPublic();
        short len = pub.getW(buffer, (short) 0);
        apdu.setOutgoingAndSend((short) 0, len);
    }

    // ========================================================================
    // INS 0x61: MFR_SET_CERT — Load manufacturer certificate
    // ========================================================================

    /**
     * Load the manufacturer's ECDSA-SHA256 signature over this card's
     * P-256 public key.  This is the "certificate" proving the card was
     * personalized by a trusted manufacturer.
     *
     * Must be called after MFR_GENERATE_KEY and before personalization
     * (first SIGN_COMMIT or JOIN_COMMIT).
     *
     * Input: DER-encoded ECDSA signature (up to 72 bytes).
     */
    private void processMfrSetCert(APDU apdu) {
        if (!mfrKeyGenerated) {
            ISOException.throwIt(SW_MFR_NOT_READY);
        }
        if (personalized) {
            ISOException.throwIt(SW_PERSONALIZED);
        }

        byte[] buffer = apdu.getBuffer();
        short dataLen = apdu.setIncomingAndReceive();

        if (dataLen < 1 || dataLen > (short) mfrCert.length) {
            ISOException.throwIt(ISO7816.SW_WRONG_LENGTH);
        }

        Util.arrayCopy(buffer, ISO7816.OFFSET_CDATA,
                       mfrCert, (short) 0, dataLen);
        mfrCertLen = dataLen;
        mfrCertSet = true;
    }

    // ========================================================================
    // INS 0x62: MFR_ATTEST — Sign a challenge with the attestation key
    // ========================================================================

    /**
     * Sign a challenge with the card's P-256 attestation key.
     *
     * Input:  32-byte challenge (typically SHA-256 of registration data).
     * Output: [1-byte sig_len] [DER ECDSA signature] [1-byte cert_len]
     *         [manufacturer certificate] [65-byte public key]
     *
     * The verifier checks:
     *   1. Manufacturer cert is valid: ECDSA_Verify(mfr_ca_pk, card_pk, cert)
     *   2. Attestation sig is valid: ECDSA_Verify(card_pk, challenge, sig)
     *
     * Requires both MFR_GENERATE_KEY and MFR_SET_CERT to have been called.
     */
    private void processMfrAttest(APDU apdu) {
        if (!mfrKeyGenerated) {
            ISOException.throwIt(SW_MFR_NOT_READY);
        }
        if (!mfrCertSet) {
            ISOException.throwIt(SW_MFR_NO_CERT);
        }

        byte[] buffer = apdu.getBuffer();
        short dataLen = apdu.setIncomingAndReceive();

        if (dataLen != 32) {
            ISOException.throwIt(ISO7816.SW_WRONG_LENGTH);
        }

        // Sign the challenge with the card's P-256 private key
        ECPrivateKey priv = (ECPrivateKey) mfrKeyPair.getPrivate();
        mfrSigner.init(priv, Signature.MODE_SIGN);
        short sigLen = mfrSigner.sign(buffer, ISO7816.OFFSET_CDATA, (short) 32,
                                       buffer, (short) 1);
        buffer[0] = (byte) sigLen;
        short off = (short)(1 + sigLen);

        // Append the manufacturer certificate
        buffer[off] = (byte) mfrCertLen;
        off++;
        Util.arrayCopy(mfrCert, (short) 0, buffer, off, mfrCertLen);
        off += mfrCertLen;

        // Append the card's public key
        ECPublicKey pub = (ECPublicKey) mfrKeyPair.getPublic();
        short pkLen = pub.getW(buffer, off);
        off += pkLen;

        apdu.setOutgoingAndSend((short) 0, off);
    }

    // ========================================================================
    // Helper methods
    // ========================================================================

    private void requireKeyInitialized() {
        if (!keyInitialized) {
            ISOException.throwIt(ISO7816.SW_CONDITIONS_NOT_SATISFIED);
        }
    }

    private void computeSchnorrResponse(byte[] challengeBuf, short challengeOff,
                                         byte[] outBuf, short outOff) {
        scalarMulAdd(rCard, (short) 0,
                     challengeBuf, challengeOff,
                     cardSk, (short) 0,
                     outBuf, outOff);
    }

    private void scalarMulAdd(byte[] aBuf, short aOff,
                               byte[] bBuf, short bOff,
                               byte[] cBuf, short cOff,
                               byte[] outBuf, short outOff) {
        ECMath.scalarMulAdd(aBuf, aOff, bBuf, bOff, cBuf, cOff, outBuf, outOff);
    }

    private void ecPointMul(byte[] pointBuf, short pointOff,
                            byte[] scalarBuf, short scalarOff,
                            byte[] outBuf, short outOff) {
        ECMath.ecPointMul(pointBuf, pointOff, scalarBuf, scalarOff, outBuf, outOff);
    }

    private void reduceModOrder(byte[] buf, short offset) {
        ECMath.reduceModOrder(buf, offset);
    }

    private void scalarNegate(byte[] inBuf, short inOff,
                               byte[] outBuf, short outOff) {
        ECMath.scalarNegate(inBuf, inOff, outBuf, outOff);
    }

    private void ecPointAdd(byte[] aBuf, short aOff,
                            byte[] bBuf, short bOff,
                            byte[] outBuf, short outOff) {
        ECMath.ecPointAdd(aBuf, aOff, bBuf, bOff, outBuf, outOff);
    }
}
