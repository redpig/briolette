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

import com.licel.jcardsim.smartcardio.CardSimulator;
import com.licel.jcardsim.utils.AIDUtil;
import javacard.framework.AID;

import javax.smartcardio.CommandAPDU;
import javax.smartcardio.ResponseAPDU;

import org.junit.Before;
import org.junit.Test;
import static org.junit.Assert.*;

/**
 * jCardSim-based tests for BrioletteApplet.
 *
 * Tests both BN254 (v0) and BLS12-381 (v1) curve modes. Each test class
 * instance gets a fresh applet, so the curve is selected per-test via
 * GENERATE_KEY with the appropriate P1 version byte.
 */
public class BrioletteAppletTest {

    private static final byte CLA = (byte) 0x80;
    private static final byte VERSION_BN254 = (byte) 0x00;
    private static final byte VERSION_BLS381 = (byte) 0x01;

    // INS codes
    private static final byte INS_GENERATE_KEY = (byte) 0x01;
    private static final byte INS_PUBLIC_KEY_SHARE = (byte) 0x02;
    private static final byte INS_SIGN_COMMIT = (byte) 0x10;
    private static final byte INS_SIGN_COMMIT_BSN = (byte) 0x11;
    private static final byte INS_SIGN_RESPOND = (byte) 0x12;
    private static final byte INS_SIGN_COMMIT_SWAP = (byte) 0x13;
    private static final byte INS_JOIN_COMMIT = (byte) 0x20;
    private static final byte INS_JOIN_RESPOND = (byte) 0x21;
    private static final byte INS_RESET_BLOOM = (byte) 0x30;
    private static final byte INS_SET_SWAP_PUBKEY = (byte) 0x31;
    private static final byte INS_GET_STATUS = (byte) 0x40;

    // Status words
    private static final int SW_OK = 0x9000;
    private static final int SW_CLA_NOT_SUPPORTED = 0x6E00;
    private static final int SW_INS_NOT_SUPPORTED = 0x6D00;
    private static final int SW_WRONG_LENGTH = 0x6700;
    private static final int SW_CONDITIONS_NOT_SATISFIED = 0x6985;
    private static final int SW_BASENAME_USED = 0x6A84;
    private static final int SW_SWAP_AUTH_FAILED = 0x6A85;
    private static final int SW_BAD_VERSION = 0x6A86;
    private static final int SW_NO_SWAP_KEY = 0x6A87;

    private static final int FR_BYTES = 32;
    private static final int BN254_G1_BYTES = 65;
    private static final int BLS381_G1_BYTES = 48;

    private CardSimulator simulator;

    /** A dummy BN254 uncompressed G1 point (0x04 prefix + 64 bytes). */
    private byte[] dummyBN254Point;

    /** A dummy BLS12-381 compressed G1 point (48 bytes, compression flag set). */
    private byte[] dummyBLS381Point;

    /** A dummy Fr scalar (32 bytes). */
    private byte[] dummyScalar;

    @Before
    public void setUp() {
        simulator = new CardSimulator();

        AID appletAID = AIDUtil.create("4272696F6C6574746501");
        simulator.installApplet(appletAID, BrioletteApplet.class);
        simulator.selectApplet(appletAID);

        // Build a dummy BN254 G1 point: 0x04 || 32 bytes x || 32 bytes y
        dummyBN254Point = new byte[BN254_G1_BYTES];
        dummyBN254Point[0] = 0x04;
        for (int i = 1; i < BN254_G1_BYTES; i++) {
            dummyBN254Point[i] = (byte) (i & 0xFF);
        }

        // Build a dummy BLS12-381 compressed G1 point (48 bytes)
        // Use the standard generator point for valid curve operations
        dummyBLS381Point = new byte[BLS381_G1_BYTES];
        System.arraycopy(BLS12381Params.G1_COMPRESSED, 0,
                         dummyBLS381Point, 0, BLS381_G1_BYTES);

        // Build a dummy scalar
        dummyScalar = new byte[FR_BYTES];
        for (int i = 0; i < FR_BYTES; i++) {
            dummyScalar[i] = (byte) ((i + 0x10) & 0xFF);
        }
    }

    // ========================================================================
    // Helpers
    // ========================================================================

    private ResponseAPDU send(byte ins, byte p1, byte[] data) {
        CommandAPDU cmd;
        if (data != null && data.length > 0) {
            cmd = new CommandAPDU(CLA, ins, p1, 0x00, data, 256);
        } else {
            cmd = new CommandAPDU(CLA, ins, p1, 0x00, 256);
        }
        return simulator.transmitCommand(cmd);
    }

    private ResponseAPDU send(byte ins, byte[] data) {
        return send(ins, VERSION_BN254, data);
    }

    private ResponseAPDU send(byte ins) {
        return send(ins, VERSION_BN254, null);
    }

    private void generateKey() {
        generateKey(VERSION_BN254);
    }

    private void generateKey(byte version) {
        ResponseAPDU resp = send(INS_GENERATE_KEY, version, null);
        assertEquals("GENERATE_KEY should succeed", SW_OK, resp.getSW());
    }

    /** Get the G1 point size for the given curve version. */
    private int g1Bytes(byte version) {
        return version == VERSION_BN254 ? BN254_G1_BYTES : BLS381_G1_BYTES;
    }

    /** Get the dummy G1 point for the given curve version. */
    private byte[] dummyPoint(byte version) {
        return version == VERSION_BN254 ? dummyBN254Point : dummyBLS381Point;
    }

    // ========================================================================
    // GET_STATUS tests
    // ========================================================================

    @Test
    public void testGetStatus_initialState() {
        ResponseAPDU resp = send(INS_GET_STATUS);
        assertEquals(SW_OK, resp.getSW());

        byte[] data = resp.getData();
        assertEquals("Status should be 6 bytes", 6, data.length);

        byte flags = data[0];
        assertEquals("Key should not be initialized", 0, flags & 0x01);
        assertEquals("No active session", 0, flags & 0x02);
        assertEquals("Swap key not set", 0, flags & 0x04);

        int epoch = ((data[1] & 0xFF) << 24) | ((data[2] & 0xFF) << 16)
                  | ((data[3] & 0xFF) << 8) | (data[4] & 0xFF);
        assertEquals("Initial epoch should be 0", 0, epoch);

        assertEquals("Default version should be BN254", VERSION_BN254, data[5]);
    }

    @Test
    public void testGetStatus_afterKeyGen() {
        generateKey();

        ResponseAPDU resp = send(INS_GET_STATUS);
        assertEquals(SW_OK, resp.getSW());

        byte flags = resp.getData()[0];
        assertEquals("Key should be initialized", 1, flags & 0x01);
    }

    @Test
    public void testGetStatus_ignoresCurveVersion() {
        ResponseAPDU resp = send(INS_GET_STATUS, VERSION_BLS381, null);
        assertEquals(SW_OK, resp.getSW());
        assertEquals(6, resp.getData().length);
    }

    @Test
    public void testGetStatus_reportsBLS381Version() {
        generateKey(VERSION_BLS381);

        ResponseAPDU resp = send(INS_GET_STATUS);
        assertEquals(SW_OK, resp.getSW());
        assertEquals("Should report BLS12-381", VERSION_BLS381, resp.getData()[5]);
    }

    // ========================================================================
    // GENERATE_KEY tests
    // ========================================================================

    @Test
    public void testGenerateKey_success() {
        ResponseAPDU resp = send(INS_GENERATE_KEY);
        assertEquals(SW_OK, resp.getSW());
    }

    @Test
    public void testGenerateKey_BLS381_success() {
        ResponseAPDU resp = send(INS_GENERATE_KEY, VERSION_BLS381, null);
        assertEquals(SW_OK, resp.getSW());
    }

    @Test
    public void testGenerateKey_doubleCallFails() {
        generateKey();

        ResponseAPDU resp = send(INS_GENERATE_KEY);
        assertEquals("Second GENERATE_KEY should fail",
                     SW_CONDITIONS_NOT_SATISFIED, resp.getSW());
    }

    @Test
    public void testGenerateKey_badVersionRejected() {
        ResponseAPDU resp = send(INS_GENERATE_KEY, (byte) 0x02, null);
        assertEquals("Invalid version should fail", SW_BAD_VERSION, resp.getSW());
    }

    // ========================================================================
    // PUBLIC_KEY_SHARE tests (BN254)
    // ========================================================================

    @Test
    public void testPublicKeyShare_success() {
        generateKey();

        ResponseAPDU resp = send(INS_PUBLIC_KEY_SHARE, dummyBN254Point);
        assertEquals(SW_OK, resp.getSW());
        assertEquals("Should return a G1 point",
                     BN254_G1_BYTES, resp.getData().length);
    }

    @Test
    public void testPublicKeyShare_beforeKeyGenFails() {
        // Before keygen, no curve is active — should fail with CONDITIONS
        ResponseAPDU resp = send(INS_PUBLIC_KEY_SHARE, dummyBN254Point);
        assertEquals(SW_CONDITIONS_NOT_SATISFIED, resp.getSW());
    }

    @Test
    public void testPublicKeyShare_wrongLengthFails() {
        generateKey();

        byte[] tooShort = new byte[32];
        ResponseAPDU resp = send(INS_PUBLIC_KEY_SHARE, tooShort);
        assertEquals(SW_WRONG_LENGTH, resp.getSW());
    }

    // ========================================================================
    // PUBLIC_KEY_SHARE tests (BLS12-381)
    // ========================================================================

    @Test
    public void testPublicKeyShare_BLS381_success() {
        generateKey(VERSION_BLS381);

        ResponseAPDU resp = send(INS_PUBLIC_KEY_SHARE, VERSION_BLS381, dummyBLS381Point);
        assertEquals(SW_OK, resp.getSW());
        assertEquals("Should return compressed G1 point",
                     BLS381_G1_BYTES, resp.getData().length);
    }

    @Test
    public void testPublicKeyShare_BLS381_wrongVersionFails() {
        generateKey(VERSION_BLS381);

        // Send with BN254 version byte — should fail
        ResponseAPDU resp = send(INS_PUBLIC_KEY_SHARE, VERSION_BN254, dummyBN254Point);
        assertEquals("Version mismatch should fail", SW_BAD_VERSION, resp.getSW());
    }

    // ========================================================================
    // SIGN_COMMIT (no basename) tests
    // ========================================================================

    @Test
    public void testSignCommit_noBasename_success() {
        generateKey();

        ResponseAPDU resp = send(INS_SIGN_COMMIT, dummyBN254Point);
        assertEquals(SW_OK, resp.getSW());
        assertEquals("Should return U_card (G1 point)",
                     BN254_G1_BYTES, resp.getData().length);
    }

    @Test
    public void testSignCommit_BLS381_noBasename_success() {
        generateKey(VERSION_BLS381);

        ResponseAPDU resp = send(INS_SIGN_COMMIT, VERSION_BLS381, dummyBLS381Point);
        assertEquals(SW_OK, resp.getSW());
        assertEquals("Should return compressed U_card",
                     BLS381_G1_BYTES, resp.getData().length);
    }

    @Test
    public void testSignCommit_noBasename_wrongLength() {
        generateKey();

        byte[] tooShort = new byte[32];
        ResponseAPDU resp = send(INS_SIGN_COMMIT, tooShort);
        assertEquals(SW_WRONG_LENGTH, resp.getSW());
    }

    @Test
    public void testSignCommit_noBasename_beforeKeyGenFails() {
        ResponseAPDU resp = send(INS_SIGN_COMMIT, dummyBN254Point);
        assertEquals(SW_CONDITIONS_NOT_SATISFIED, resp.getSW());
    }

    // ========================================================================
    // SIGN_COMMIT_BSN (with basename) tests
    // ========================================================================

    @Test
    public void testSignCommitBsn_success() {
        generateKey();

        byte[] input = new byte[BN254_G1_BYTES * 2];
        System.arraycopy(dummyBN254Point, 0, input, 0, BN254_G1_BYTES);
        System.arraycopy(dummyBN254Point, 0, input, BN254_G1_BYTES, BN254_G1_BYTES);

        ResponseAPDU resp = send(INS_SIGN_COMMIT_BSN, input);
        assertEquals(SW_OK, resp.getSW());
        assertEquals("Should return 3 G1 points",
                     BN254_G1_BYTES * 3, resp.getData().length);
    }

    @Test
    public void testSignCommitBsn_BLS381_success() {
        generateKey(VERSION_BLS381);

        byte[] input = new byte[BLS381_G1_BYTES * 2];
        System.arraycopy(dummyBLS381Point, 0, input, 0, BLS381_G1_BYTES);
        System.arraycopy(dummyBLS381Point, 0, input, BLS381_G1_BYTES, BLS381_G1_BYTES);

        ResponseAPDU resp = send(INS_SIGN_COMMIT_BSN, VERSION_BLS381, input);
        assertEquals(SW_OK, resp.getSW());
        assertEquals("Should return 3 compressed G1 points",
                     BLS381_G1_BYTES * 3, resp.getData().length);
    }

    @Test
    public void testSignCommitBsn_doubleSpendBlocked() {
        generateKey();

        byte[] input = new byte[BN254_G1_BYTES * 2];
        System.arraycopy(dummyBN254Point, 0, input, 0, BN254_G1_BYTES);
        System.arraycopy(dummyBN254Point, 0, input, BN254_G1_BYTES, BN254_G1_BYTES);

        ResponseAPDU resp1 = send(INS_SIGN_COMMIT_BSN, input);
        assertEquals("First sign should succeed", SW_OK, resp1.getSW());

        ResponseAPDU resp2 = send(INS_SIGN_COMMIT_BSN, input);
        assertEquals("Same basename should be rejected", SW_BASENAME_USED, resp2.getSW());
    }

    @Test
    public void testSignCommitBsn_differentBasenamesSucceed() {
        generateKey();

        byte[] input1 = new byte[BN254_G1_BYTES * 2];
        System.arraycopy(dummyBN254Point, 0, input1, 0, BN254_G1_BYTES);
        System.arraycopy(dummyBN254Point, 0, input1, BN254_G1_BYTES, BN254_G1_BYTES);

        ResponseAPDU resp1 = send(INS_SIGN_COMMIT_BSN, input1);
        assertEquals(SW_OK, resp1.getSW());

        byte[] input2 = new byte[BN254_G1_BYTES * 2];
        System.arraycopy(dummyBN254Point, 0, input2, 0, BN254_G1_BYTES);
        input2[BN254_G1_BYTES] = 0x04;
        for (int i = BN254_G1_BYTES + 1; i < BN254_G1_BYTES * 2; i++) {
            input2[i] = (byte) 0xFF;
        }

        ResponseAPDU resp2 = send(INS_SIGN_COMMIT_BSN, input2);
        assertEquals("Different basename should succeed", SW_OK, resp2.getSW());
    }

    @Test
    public void testSignCommitBsn_wrongLength() {
        generateKey();

        byte[] tooShort = new byte[100];
        ResponseAPDU resp = send(INS_SIGN_COMMIT_BSN, tooShort);
        assertEquals(SW_WRONG_LENGTH, resp.getSW());
    }

    // ========================================================================
    // SIGN_COMMIT_SWAP tests
    // ========================================================================

    @Test
    public void testSignCommitSwap_noSwapKeyFails() {
        generateKey();

        byte[] input = new byte[BN254_G1_BYTES * 2 + 64];
        System.arraycopy(dummyBN254Point, 0, input, 0, BN254_G1_BYTES);
        System.arraycopy(dummyBN254Point, 0, input, BN254_G1_BYTES, BN254_G1_BYTES);

        ResponseAPDU resp = send(INS_SIGN_COMMIT_SWAP, input);
        assertEquals("Should fail without swap pubkey", SW_NO_SWAP_KEY, resp.getSW());
    }

    @Test
    public void testSignCommitSwap_invalidAuthFails() {
        generateKey();

        ResponseAPDU setKey = send(INS_SET_SWAP_PUBKEY, dummyBN254Point);
        assertEquals(SW_OK, setKey.getSW());

        byte[] input = new byte[BN254_G1_BYTES * 2 + 64];
        System.arraycopy(dummyBN254Point, 0, input, 0, BN254_G1_BYTES);
        System.arraycopy(dummyBN254Point, 0, input, BN254_G1_BYTES, BN254_G1_BYTES);

        ResponseAPDU resp = send(INS_SIGN_COMMIT_SWAP, input);
        assertEquals("Should fail with invalid auth",
                     SW_SWAP_AUTH_FAILED, resp.getSW());
    }

    @Test
    public void testSignCommitSwap_wrongLength() {
        generateKey();

        byte[] tooShort = new byte[100];
        ResponseAPDU resp = send(INS_SIGN_COMMIT_SWAP, tooShort);
        assertEquals(SW_WRONG_LENGTH, resp.getSW());
    }

    // ========================================================================
    // SIGN_RESPOND tests
    // ========================================================================

    @Test
    public void testSignRespond_afterCommit_success() {
        generateKey();

        ResponseAPDU commit = send(INS_SIGN_COMMIT, dummyBN254Point);
        assertEquals(SW_OK, commit.getSW());

        ResponseAPDU resp = send(INS_SIGN_RESPOND, dummyScalar);
        assertEquals(SW_OK, resp.getSW());
        assertEquals("Should return s_card scalar", FR_BYTES, resp.getData().length);
    }

    @Test
    public void testSignRespond_BLS381_afterCommit_success() {
        generateKey(VERSION_BLS381);

        ResponseAPDU commit = send(INS_SIGN_COMMIT, VERSION_BLS381, dummyBLS381Point);
        assertEquals(SW_OK, commit.getSW());

        ResponseAPDU resp = send(INS_SIGN_RESPOND, VERSION_BLS381, dummyScalar);
        assertEquals(SW_OK, resp.getSW());
        assertEquals("Should return s_card scalar", FR_BYTES, resp.getData().length);
    }

    @Test
    public void testSignRespond_withoutCommitFails() {
        generateKey();

        ResponseAPDU resp = send(INS_SIGN_RESPOND, dummyScalar);
        assertEquals("Should fail without prior commit",
                     SW_CONDITIONS_NOT_SATISFIED, resp.getSW());
    }

    @Test
    public void testSignRespond_doubleRespondFails() {
        generateKey();

        send(INS_SIGN_COMMIT, dummyBN254Point);

        ResponseAPDU resp1 = send(INS_SIGN_RESPOND, dummyScalar);
        assertEquals(SW_OK, resp1.getSW());

        ResponseAPDU resp2 = send(INS_SIGN_RESPOND, dummyScalar);
        assertEquals("Double respond should fail",
                     SW_CONDITIONS_NOT_SATISFIED, resp2.getSW());
    }

    @Test
    public void testSignRespond_wrongLength() {
        generateKey();
        send(INS_SIGN_COMMIT, dummyBN254Point);

        byte[] tooShort = new byte[16];
        ResponseAPDU resp = send(INS_SIGN_RESPOND, tooShort);
        assertEquals(SW_WRONG_LENGTH, resp.getSW());
    }

    @Test
    public void testSignRespond_beforeKeyGenFails() {
        ResponseAPDU resp = send(INS_SIGN_RESPOND, dummyScalar);
        assertEquals(SW_CONDITIONS_NOT_SATISFIED, resp.getSW());
    }

    // ========================================================================
    // JOIN_COMMIT / JOIN_RESPOND tests
    // ========================================================================

    @Test
    public void testJoinCommit_success() {
        generateKey();

        ResponseAPDU resp = send(INS_JOIN_COMMIT, dummyBN254Point);
        assertEquals(SW_OK, resp.getSW());
        assertEquals("Should return U_card", BN254_G1_BYTES, resp.getData().length);
    }

    @Test
    public void testJoinCommit_BLS381_success() {
        generateKey(VERSION_BLS381);

        ResponseAPDU resp = send(INS_JOIN_COMMIT, VERSION_BLS381, dummyBLS381Point);
        assertEquals(SW_OK, resp.getSW());
        assertEquals("Should return compressed U_card",
                     BLS381_G1_BYTES, resp.getData().length);
    }

    @Test
    public void testJoinCommit_beforeKeyGenFails() {
        ResponseAPDU resp = send(INS_JOIN_COMMIT, dummyBN254Point);
        assertEquals(SW_CONDITIONS_NOT_SATISFIED, resp.getSW());
    }

    @Test
    public void testJoinCommit_wrongLength() {
        generateKey();

        byte[] tooShort = new byte[32];
        ResponseAPDU resp = send(INS_JOIN_COMMIT, tooShort);
        assertEquals(SW_WRONG_LENGTH, resp.getSW());
    }

    @Test
    public void testJoinRespond_afterCommit_success() {
        generateKey();

        ResponseAPDU commit = send(INS_JOIN_COMMIT, dummyBN254Point);
        assertEquals(SW_OK, commit.getSW());

        ResponseAPDU resp = send(INS_JOIN_RESPOND, dummyScalar);
        assertEquals(SW_OK, resp.getSW());
        assertEquals("Should return s_card scalar", FR_BYTES, resp.getData().length);
    }

    @Test
    public void testJoinRespond_withoutCommitFails() {
        generateKey();

        ResponseAPDU resp = send(INS_JOIN_RESPOND, dummyScalar);
        assertEquals(SW_CONDITIONS_NOT_SATISFIED, resp.getSW());
    }

    @Test
    public void testJoinRespond_doubleRespondFails() {
        generateKey();

        send(INS_JOIN_COMMIT, dummyBN254Point);
        ResponseAPDU resp1 = send(INS_JOIN_RESPOND, dummyScalar);
        assertEquals(SW_OK, resp1.getSW());

        ResponseAPDU resp2 = send(INS_JOIN_RESPOND, dummyScalar);
        assertEquals(SW_CONDITIONS_NOT_SATISFIED, resp2.getSW());
    }

    @Test
    public void testJoinRespond_wrongLength() {
        generateKey();
        send(INS_JOIN_COMMIT, dummyBN254Point);

        byte[] tooShort = new byte[16];
        ResponseAPDU resp = send(INS_JOIN_RESPOND, tooShort);
        assertEquals(SW_WRONG_LENGTH, resp.getSW());
    }

    // ========================================================================
    // Session type isolation tests
    // ========================================================================

    @Test
    public void testSignRespondRejectsJoinSession() {
        generateKey();
        send(INS_JOIN_COMMIT, dummyBN254Point);

        ResponseAPDU resp = send(INS_SIGN_RESPOND, dummyScalar);
        assertEquals("Sign respond should reject join session",
                     SW_CONDITIONS_NOT_SATISFIED, resp.getSW());
    }

    @Test
    public void testJoinRespondRejectsSignSession() {
        generateKey();
        send(INS_SIGN_COMMIT, dummyBN254Point);

        ResponseAPDU resp = send(INS_JOIN_RESPOND, dummyScalar);
        assertEquals("Join respond should reject sign session",
                     SW_CONDITIONS_NOT_SATISFIED, resp.getSW());
    }

    // ========================================================================
    // RESET_BLOOM tests
    // ========================================================================

    @Test
    public void testResetBloom_success() {
        byte[] epoch1 = {0x00, 0x00, 0x00, 0x01};
        ResponseAPDU resp = send(INS_RESET_BLOOM, VERSION_BN254, epoch1);
        assertEquals(SW_OK, resp.getSW());
    }

    @Test
    public void testResetBloom_monotonic() {
        byte[] epoch2 = {0x00, 0x00, 0x00, 0x02};
        ResponseAPDU resp1 = send(INS_RESET_BLOOM, VERSION_BN254, epoch2);
        assertEquals(SW_OK, resp1.getSW());

        ResponseAPDU resp2 = send(INS_RESET_BLOOM, VERSION_BN254, epoch2);
        assertEquals("Same epoch should fail",
                     SW_CONDITIONS_NOT_SATISFIED, resp2.getSW());

        byte[] epoch1 = {0x00, 0x00, 0x00, 0x01};
        ResponseAPDU resp3 = send(INS_RESET_BLOOM, VERSION_BN254, epoch1);
        assertEquals("Lower epoch should fail",
                     SW_CONDITIONS_NOT_SATISFIED, resp3.getSW());
    }

    @Test
    public void testResetBloom_clearsBloomFilter() {
        generateKey();

        byte[] input = new byte[BN254_G1_BYTES * 2];
        System.arraycopy(dummyBN254Point, 0, input, 0, BN254_G1_BYTES);
        System.arraycopy(dummyBN254Point, 0, input, BN254_G1_BYTES, BN254_G1_BYTES);

        ResponseAPDU resp1 = send(INS_SIGN_COMMIT_BSN, input);
        assertEquals(SW_OK, resp1.getSW());

        ResponseAPDU resp2 = send(INS_SIGN_COMMIT_BSN, input);
        assertEquals(SW_BASENAME_USED, resp2.getSW());

        byte[] epoch1 = {0x00, 0x00, 0x00, 0x01};
        send(INS_RESET_BLOOM, VERSION_BN254, epoch1);

        ResponseAPDU resp3 = send(INS_SIGN_COMMIT_BSN, input);
        assertEquals("Basename should succeed after bloom reset", SW_OK, resp3.getSW());
    }

    @Test
    public void testResetBloom_wrongLength() {
        byte[] tooShort = {0x01, 0x02};
        ResponseAPDU resp = send(INS_RESET_BLOOM, VERSION_BN254, tooShort);
        assertEquals(SW_WRONG_LENGTH, resp.getSW());
    }

    @Test
    public void testResetBloom_updatesEpochInStatus() {
        byte[] epoch5 = {0x00, 0x00, 0x00, 0x05};
        send(INS_RESET_BLOOM, VERSION_BN254, epoch5);

        ResponseAPDU status = send(INS_GET_STATUS);
        byte[] data = status.getData();
        int epoch = ((data[1] & 0xFF) << 24) | ((data[2] & 0xFF) << 16)
                  | ((data[3] & 0xFF) << 8) | (data[4] & 0xFF);
        assertEquals("Epoch should be 5", 5, epoch);
    }

    // ========================================================================
    // SET_SWAP_PUBKEY tests
    // ========================================================================

    @Test
    public void testSetSwapPubkey_success() {
        generateKey();
        ResponseAPDU resp = send(INS_SET_SWAP_PUBKEY, dummyBN254Point);
        assertEquals(SW_OK, resp.getSW());
    }

    @Test
    public void testSetSwapPubkey_BLS381_success() {
        generateKey(VERSION_BLS381);
        ResponseAPDU resp = send(INS_SET_SWAP_PUBKEY, VERSION_BLS381, dummyBLS381Point);
        assertEquals(SW_OK, resp.getSW());
    }

    @Test
    public void testSetSwapPubkey_setsStatusFlag() {
        generateKey();
        send(INS_SET_SWAP_PUBKEY, dummyBN254Point);

        ResponseAPDU status = send(INS_GET_STATUS);
        byte flags = status.getData()[0];
        assertNotEquals("Swap key flag should be set", 0, flags & 0x04);
    }

    @Test
    public void testSetSwapPubkey_wrongLength() {
        generateKey();
        byte[] tooShort = new byte[32];
        ResponseAPDU resp = send(INS_SET_SWAP_PUBKEY, tooShort);
        assertEquals(SW_WRONG_LENGTH, resp.getSW());
    }

    // ========================================================================
    // CLA / INS / version rejection tests
    // ========================================================================

    @Test
    public void testWrongCla_rejected() {
        CommandAPDU cmd = new CommandAPDU(0x00, INS_GET_STATUS, 0x00, 0x00, 256);
        ResponseAPDU resp = simulator.transmitCommand(cmd);
        assertEquals(SW_CLA_NOT_SUPPORTED, resp.getSW());
    }

    @Test
    public void testUnknownIns_rejected() {
        generateKey();
        ResponseAPDU resp = send((byte) 0xFF);
        assertEquals(SW_INS_NOT_SUPPORTED, resp.getSW());
    }

    @Test
    public void testBadCurveVersion_afterBN254KeyGen() {
        generateKey();

        ResponseAPDU resp = send(INS_PUBLIC_KEY_SHARE, VERSION_BLS381, dummyBLS381Point);
        assertEquals("BLS381 should be rejected for BN254 card",
                     SW_BAD_VERSION, resp.getSW());
    }

    @Test
    public void testBadCurveVersion_afterBLS381KeyGen() {
        generateKey(VERSION_BLS381);

        ResponseAPDU resp = send(INS_PUBLIC_KEY_SHARE, VERSION_BN254, dummyBN254Point);
        assertEquals("BN254 should be rejected for BLS381 card",
                     SW_BAD_VERSION, resp.getSW());
    }

    // ========================================================================
    // Full commit/respond flow tests
    // ========================================================================

    @Test
    public void testFullSignFlow_noBasename() {
        generateKey();

        ResponseAPDU commit = send(INS_SIGN_COMMIT, dummyBN254Point);
        assertEquals(SW_OK, commit.getSW());
        assertEquals(BN254_G1_BYTES, commit.getData().length);

        ResponseAPDU status = send(INS_GET_STATUS);
        assertNotEquals("Session should be active", 0, status.getData()[0] & 0x02);

        ResponseAPDU respond = send(INS_SIGN_RESPOND, dummyScalar);
        assertEquals(SW_OK, respond.getSW());
        assertEquals(FR_BYTES, respond.getData().length);

        status = send(INS_GET_STATUS);
        assertEquals("Session should be cleared", 0, status.getData()[0] & 0x02);
    }

    @Test
    public void testFullSignFlow_BLS381_noBasename() {
        generateKey(VERSION_BLS381);

        ResponseAPDU commit = send(INS_SIGN_COMMIT, VERSION_BLS381, dummyBLS381Point);
        assertEquals(SW_OK, commit.getSW());
        assertEquals(BLS381_G1_BYTES, commit.getData().length);

        ResponseAPDU respond = send(INS_SIGN_RESPOND, VERSION_BLS381, dummyScalar);
        assertEquals(SW_OK, respond.getSW());
        assertEquals(FR_BYTES, respond.getData().length);
    }

    @Test
    public void testFullSignFlow_withBasename() {
        generateKey();

        byte[] input = new byte[BN254_G1_BYTES * 2];
        System.arraycopy(dummyBN254Point, 0, input, 0, BN254_G1_BYTES);
        System.arraycopy(dummyBN254Point, 0, input, BN254_G1_BYTES, BN254_G1_BYTES);

        ResponseAPDU commit = send(INS_SIGN_COMMIT_BSN, input);
        assertEquals(SW_OK, commit.getSW());
        assertEquals("Should return U_card + K_card + K_u_card",
                     BN254_G1_BYTES * 3, commit.getData().length);

        ResponseAPDU respond = send(INS_SIGN_RESPOND, dummyScalar);
        assertEquals(SW_OK, respond.getSW());
        assertEquals(FR_BYTES, respond.getData().length);
    }

    @Test
    public void testFullSignFlow_BLS381_withBasename() {
        generateKey(VERSION_BLS381);

        byte[] input = new byte[BLS381_G1_BYTES * 2];
        System.arraycopy(dummyBLS381Point, 0, input, 0, BLS381_G1_BYTES);
        System.arraycopy(dummyBLS381Point, 0, input, BLS381_G1_BYTES, BLS381_G1_BYTES);

        ResponseAPDU commit = send(INS_SIGN_COMMIT_BSN, VERSION_BLS381, input);
        assertEquals(SW_OK, commit.getSW());
        assertEquals("Should return 3 compressed G1 points",
                     BLS381_G1_BYTES * 3, commit.getData().length);

        ResponseAPDU respond = send(INS_SIGN_RESPOND, VERSION_BLS381, dummyScalar);
        assertEquals(SW_OK, respond.getSW());
        assertEquals(FR_BYTES, respond.getData().length);
    }

    @Test
    public void testFullJoinFlow() {
        generateKey();

        ResponseAPDU commit = send(INS_JOIN_COMMIT, dummyBN254Point);
        assertEquals(SW_OK, commit.getSW());
        assertEquals(BN254_G1_BYTES, commit.getData().length);

        ResponseAPDU respond = send(INS_JOIN_RESPOND, dummyScalar);
        assertEquals(SW_OK, respond.getSW());
        assertEquals(FR_BYTES, respond.getData().length);
    }

    @Test
    public void testFullJoinFlow_BLS381() {
        generateKey(VERSION_BLS381);

        ResponseAPDU commit = send(INS_JOIN_COMMIT, VERSION_BLS381, dummyBLS381Point);
        assertEquals(SW_OK, commit.getSW());
        assertEquals(BLS381_G1_BYTES, commit.getData().length);

        ResponseAPDU respond = send(INS_JOIN_RESPOND, VERSION_BLS381, dummyScalar);
        assertEquals(SW_OK, respond.getSW());
        assertEquals(FR_BYTES, respond.getData().length);
    }

    @Test
    public void testMultipleSignSessions() {
        generateKey();

        send(INS_SIGN_COMMIT, dummyBN254Point);
        ResponseAPDU resp1 = send(INS_SIGN_RESPOND, dummyScalar);
        assertEquals(SW_OK, resp1.getSW());

        send(INS_SIGN_COMMIT, dummyBN254Point);
        ResponseAPDU resp2 = send(INS_SIGN_RESPOND, dummyScalar);
        assertEquals(SW_OK, resp2.getSW());
    }

    // ========================================================================
    // EC math correctness tests
    // ========================================================================

    @Test
    public void testPublicKeyShare_producesValidPoint() {
        generateKey();

        ResponseAPDU resp = send(INS_PUBLIC_KEY_SHARE, BN254Params.G1_UNCOMPRESSED);
        assertEquals(SW_OK, resp.getSW());

        byte[] q = resp.getData();
        assertEquals(BN254_G1_BYTES, q.length);
        assertEquals("Should have uncompressed prefix", 0x04, q[0] & 0xFF);

        boolean allZero = true;
        for (int i = 1; i < BN254_G1_BYTES; i++) {
            if (q[i] != 0) { allZero = false; break; }
        }
        assertFalse("Q_card should not be identity point", allZero);
    }

    @Test
    public void testPublicKeyShare_BLS381_producesValidPoint() {
        generateKey(VERSION_BLS381);

        ResponseAPDU resp = send(INS_PUBLIC_KEY_SHARE, VERSION_BLS381,
                                 BLS12381Params.G1_COMPRESSED);
        assertEquals(SW_OK, resp.getSW());

        byte[] q = resp.getData();
        assertEquals(BLS381_G1_BYTES, q.length);

        // Compressed point should have compression flag set (bit 7)
        assertTrue("Should have compression flag",
                   (q[0] & 0x80) != 0);

        // Should not be identity (identity = 0xC0 followed by zeros)
        boolean isIdentity = (q[0] & 0x40) != 0;
        assertFalse("Q_card should not be identity", isIdentity);
    }

    @Test
    public void testPublicKeyShare_deterministic() {
        generateKey();

        ResponseAPDU resp1 = send(INS_PUBLIC_KEY_SHARE, BN254Params.G1_UNCOMPRESSED);
        ResponseAPDU resp2 = send(INS_PUBLIC_KEY_SHARE, BN254Params.G1_UNCOMPRESSED);

        assertArrayEquals("Same base should give same Q_card",
                          resp1.getData(), resp2.getData());
    }

    @Test
    public void testPublicKeyShare_BLS381_deterministic() {
        generateKey(VERSION_BLS381);

        ResponseAPDU resp1 = send(INS_PUBLIC_KEY_SHARE, VERSION_BLS381,
                                  BLS12381Params.G1_COMPRESSED);
        ResponseAPDU resp2 = send(INS_PUBLIC_KEY_SHARE, VERSION_BLS381,
                                  BLS12381Params.G1_COMPRESSED);

        assertArrayEquals("Same base should give same Q_card",
                          resp1.getData(), resp2.getData());
    }

    @Test
    public void testSignRespond_producesNonTrivialScalar() {
        generateKey();

        send(INS_SIGN_COMMIT, dummyBN254Point);
        ResponseAPDU resp = send(INS_SIGN_RESPOND, dummyScalar);
        assertEquals(SW_OK, resp.getSW());

        byte[] sCard = resp.getData();
        boolean allZero = true;
        for (byte b : sCard) {
            if (b != 0) { allZero = false; break; }
        }
        assertFalse("s_card should not be zero", allZero);
    }

    @Test
    public void testSignCommit_producesDistinctPointsPerSession() {
        generateKey();

        ResponseAPDU commit1 = send(INS_SIGN_COMMIT, dummyBN254Point);
        send(INS_SIGN_RESPOND, dummyScalar);

        ResponseAPDU commit2 = send(INS_SIGN_COMMIT, dummyBN254Point);

        assertFalse("Different sessions should produce different U_card",
                    java.util.Arrays.equals(commit1.getData(), commit2.getData()));
    }

    @Test
    public void testCommitOverwritesPreviousSession() {
        generateKey();

        send(INS_SIGN_COMMIT, dummyBN254Point);
        send(INS_SIGN_COMMIT, dummyBN254Point);

        ResponseAPDU resp = send(INS_SIGN_RESPOND, dummyScalar);
        assertEquals(SW_OK, resp.getSW());
    }
}
