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
 * These tests exercise the APDU protocol, state machine transitions, error
 * handling, and bloom filter double-spend detection against the applet running
 * in the jCardSim simulator. The underlying EC math uses stub implementations,
 * so these tests focus on protocol correctness rather than cryptographic
 * correctness (which is covered by the Rust MockCard tests).
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
    private static final int G1_BYTES = 65;

    private CardSimulator simulator;

    /** A dummy uncompressed G1 point (0x04 prefix + 64 bytes). */
    private byte[] dummyG1Point;

    /** A dummy Fr scalar (32 bytes). */
    private byte[] dummyScalar;

    @Before
    public void setUp() {
        simulator = new CardSimulator();

        AID appletAID = AIDUtil.create("4272696F6C6574746501");
        simulator.installApplet(appletAID, BrioletteApplet.class);
        simulator.selectApplet(appletAID);

        // Build a dummy G1 point: 0x04 || 32 bytes x || 32 bytes y
        dummyG1Point = new byte[G1_BYTES];
        dummyG1Point[0] = 0x04;
        for (int i = 1; i < G1_BYTES; i++) {
            dummyG1Point[i] = (byte) (i & 0xFF);
        }

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
        ResponseAPDU resp = send(INS_GENERATE_KEY);
        assertEquals("GENERATE_KEY should succeed", SW_OK, resp.getSW());
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

        // Epoch should be 0
        int epoch = ((data[1] & 0xFF) << 24) | ((data[2] & 0xFF) << 16)
                  | ((data[3] & 0xFF) << 8) | (data[4] & 0xFF);
        assertEquals("Initial epoch should be 0", 0, epoch);

        assertEquals("Supported version should be BN254", VERSION_BN254, data[5]);
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
        // GET_STATUS should work regardless of P1 value
        ResponseAPDU resp = send(INS_GET_STATUS, VERSION_BLS381, null);
        assertEquals(SW_OK, resp.getSW());
        assertEquals(6, resp.getData().length);
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
    public void testGenerateKey_doubleCallFails() {
        generateKey();

        ResponseAPDU resp = send(INS_GENERATE_KEY);
        assertEquals("Second GENERATE_KEY should fail",
                     SW_CONDITIONS_NOT_SATISFIED, resp.getSW());
    }

    @Test
    public void testGenerateKey_badVersionRejected() {
        ResponseAPDU resp = send(INS_GENERATE_KEY, VERSION_BLS381, null);
        assertEquals("BLS12-381 not supported", SW_BAD_VERSION, resp.getSW());
    }

    // ========================================================================
    // PUBLIC_KEY_SHARE tests
    // ========================================================================

    @Test
    public void testPublicKeyShare_success() {
        generateKey();

        ResponseAPDU resp = send(INS_PUBLIC_KEY_SHARE, dummyG1Point);
        assertEquals(SW_OK, resp.getSW());
        assertEquals("Should return a G1 point", G1_BYTES, resp.getData().length);
    }

    @Test
    public void testPublicKeyShare_beforeKeyGenFails() {
        ResponseAPDU resp = send(INS_PUBLIC_KEY_SHARE, dummyG1Point);
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
    // SIGN_COMMIT (no basename) tests
    // ========================================================================

    @Test
    public void testSignCommit_noBasename_success() {
        generateKey();

        ResponseAPDU resp = send(INS_SIGN_COMMIT, dummyG1Point);
        assertEquals(SW_OK, resp.getSW());
        assertEquals("Should return U_card (G1 point)", G1_BYTES, resp.getData().length);
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
        ResponseAPDU resp = send(INS_SIGN_COMMIT, dummyG1Point);
        assertEquals(SW_CONDITIONS_NOT_SATISFIED, resp.getSW());
    }

    // ========================================================================
    // SIGN_COMMIT_BSN (with basename) tests
    // ========================================================================

    @Test
    public void testSignCommitBsn_success() {
        generateKey();

        // Input: S(65) + bsn_base(65) = 130 bytes
        byte[] input = new byte[G1_BYTES * 2];
        System.arraycopy(dummyG1Point, 0, input, 0, G1_BYTES);
        System.arraycopy(dummyG1Point, 0, input, G1_BYTES, G1_BYTES);

        ResponseAPDU resp = send(INS_SIGN_COMMIT_BSN, input);
        assertEquals(SW_OK, resp.getSW());
        // Output: U_card(65) + K_card(65) + K_u_card(65) = 195
        assertEquals("Should return 3 G1 points", G1_BYTES * 3, resp.getData().length);
    }

    @Test
    public void testSignCommitBsn_doubleSpendBlocked() {
        generateKey();

        byte[] input = new byte[G1_BYTES * 2];
        System.arraycopy(dummyG1Point, 0, input, 0, G1_BYTES);
        System.arraycopy(dummyG1Point, 0, input, G1_BYTES, G1_BYTES);

        ResponseAPDU resp1 = send(INS_SIGN_COMMIT_BSN, input);
        assertEquals("First sign should succeed", SW_OK, resp1.getSW());

        // Same basename again should be blocked by bloom filter
        ResponseAPDU resp2 = send(INS_SIGN_COMMIT_BSN, input);
        assertEquals("Same basename should be rejected", SW_BASENAME_USED, resp2.getSW());
    }

    @Test
    public void testSignCommitBsn_differentBasenamesSucceed() {
        generateKey();

        byte[] input1 = new byte[G1_BYTES * 2];
        System.arraycopy(dummyG1Point, 0, input1, 0, G1_BYTES);
        System.arraycopy(dummyG1Point, 0, input1, G1_BYTES, G1_BYTES);

        ResponseAPDU resp1 = send(INS_SIGN_COMMIT_BSN, input1);
        assertEquals(SW_OK, resp1.getSW());

        // Different basename point
        byte[] input2 = new byte[G1_BYTES * 2];
        System.arraycopy(dummyG1Point, 0, input2, 0, G1_BYTES);
        input2[G1_BYTES] = 0x04;
        for (int i = G1_BYTES + 1; i < G1_BYTES * 2; i++) {
            input2[i] = (byte) 0xFF;  // different from dummyG1Point
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

        // Input: S(65) + bsn_base(65) + auth_c(32) + auth_s(32) = 194
        byte[] input = new byte[G1_BYTES * 2 + 64];
        System.arraycopy(dummyG1Point, 0, input, 0, G1_BYTES);
        System.arraycopy(dummyG1Point, 0, input, G1_BYTES, G1_BYTES);

        ResponseAPDU resp = send(INS_SIGN_COMMIT_SWAP, input);
        assertEquals("Should fail without swap pubkey", SW_NO_SWAP_KEY, resp.getSW());
    }

    @Test
    public void testSignCommitSwap_invalidAuthFails() {
        generateKey();

        // Set swap pubkey first
        ResponseAPDU setKey = send(INS_SET_SWAP_PUBKEY, dummyG1Point);
        assertEquals(SW_OK, setKey.getSW());

        // Send swap commit with dummy (invalid) auth token
        byte[] input = new byte[G1_BYTES * 2 + 64];
        System.arraycopy(dummyG1Point, 0, input, 0, G1_BYTES);
        System.arraycopy(dummyG1Point, 0, input, G1_BYTES, G1_BYTES);

        ResponseAPDU resp = send(INS_SIGN_COMMIT_SWAP, input);
        // Real EC math: dummy auth token doesn't verify against swap pubkey
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

        // First, do a sign commit to establish session
        ResponseAPDU commit = send(INS_SIGN_COMMIT, dummyG1Point);
        assertEquals(SW_OK, commit.getSW());

        // Now respond with a challenge
        ResponseAPDU resp = send(INS_SIGN_RESPOND, dummyScalar);
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

        send(INS_SIGN_COMMIT, dummyG1Point);

        ResponseAPDU resp1 = send(INS_SIGN_RESPOND, dummyScalar);
        assertEquals(SW_OK, resp1.getSW());

        // Second respond without new commit should fail
        ResponseAPDU resp2 = send(INS_SIGN_RESPOND, dummyScalar);
        assertEquals("Double respond should fail",
                     SW_CONDITIONS_NOT_SATISFIED, resp2.getSW());
    }

    @Test
    public void testSignRespond_wrongLength() {
        generateKey();
        send(INS_SIGN_COMMIT, dummyG1Point);

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

        ResponseAPDU resp = send(INS_JOIN_COMMIT, dummyG1Point);
        assertEquals(SW_OK, resp.getSW());
        assertEquals("Should return U_card", G1_BYTES, resp.getData().length);
    }

    @Test
    public void testJoinCommit_beforeKeyGenFails() {
        ResponseAPDU resp = send(INS_JOIN_COMMIT, dummyG1Point);
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

        ResponseAPDU commit = send(INS_JOIN_COMMIT, dummyG1Point);
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

        send(INS_JOIN_COMMIT, dummyG1Point);
        ResponseAPDU resp1 = send(INS_JOIN_RESPOND, dummyScalar);
        assertEquals(SW_OK, resp1.getSW());

        ResponseAPDU resp2 = send(INS_JOIN_RESPOND, dummyScalar);
        assertEquals(SW_CONDITIONS_NOT_SATISFIED, resp2.getSW());
    }

    @Test
    public void testJoinRespond_wrongLength() {
        generateKey();
        send(INS_JOIN_COMMIT, dummyG1Point);

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

        // Start a join session
        send(INS_JOIN_COMMIT, dummyG1Point);

        // Try to respond with sign (wrong session type)
        ResponseAPDU resp = send(INS_SIGN_RESPOND, dummyScalar);
        assertEquals("Sign respond should reject join session",
                     SW_CONDITIONS_NOT_SATISFIED, resp.getSW());
    }

    @Test
    public void testJoinRespondRejectsSignSession() {
        generateKey();

        // Start a sign session
        send(INS_SIGN_COMMIT, dummyG1Point);

        // Try to respond with join (wrong session type)
        ResponseAPDU resp = send(INS_JOIN_RESPOND, dummyScalar);
        assertEquals("Join respond should reject sign session",
                     SW_CONDITIONS_NOT_SATISFIED, resp.getSW());
    }

    // ========================================================================
    // RESET_BLOOM tests
    // ========================================================================

    @Test
    public void testResetBloom_success() {
        // Epoch 1 should succeed (> initial 0)
        byte[] epoch1 = {0x00, 0x00, 0x00, 0x01};
        ResponseAPDU resp = send(INS_RESET_BLOOM, VERSION_BN254, epoch1);
        assertEquals(SW_OK, resp.getSW());
    }

    @Test
    public void testResetBloom_monotonic() {
        byte[] epoch2 = {0x00, 0x00, 0x00, 0x02};
        ResponseAPDU resp1 = send(INS_RESET_BLOOM, VERSION_BN254, epoch2);
        assertEquals(SW_OK, resp1.getSW());

        // Same epoch should fail
        ResponseAPDU resp2 = send(INS_RESET_BLOOM, VERSION_BN254, epoch2);
        assertEquals("Same epoch should fail",
                     SW_CONDITIONS_NOT_SATISFIED, resp2.getSW());

        // Lower epoch should fail
        byte[] epoch1 = {0x00, 0x00, 0x00, 0x01};
        ResponseAPDU resp3 = send(INS_RESET_BLOOM, VERSION_BN254, epoch1);
        assertEquals("Lower epoch should fail",
                     SW_CONDITIONS_NOT_SATISFIED, resp3.getSW());
    }

    @Test
    public void testResetBloom_clearsBloomFilter() {
        generateKey();

        byte[] input = new byte[G1_BYTES * 2];
        System.arraycopy(dummyG1Point, 0, input, 0, G1_BYTES);
        System.arraycopy(dummyG1Point, 0, input, G1_BYTES, G1_BYTES);

        // Sign with a basename
        ResponseAPDU resp1 = send(INS_SIGN_COMMIT_BSN, input);
        assertEquals(SW_OK, resp1.getSW());

        // Same basename should be blocked
        ResponseAPDU resp2 = send(INS_SIGN_COMMIT_BSN, input);
        assertEquals(SW_BASENAME_USED, resp2.getSW());

        // Reset bloom for new epoch
        byte[] epoch1 = {0x00, 0x00, 0x00, 0x01};
        send(INS_RESET_BLOOM, VERSION_BN254, epoch1);

        // Same basename should now succeed after reset
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
        ResponseAPDU resp = send(INS_SET_SWAP_PUBKEY, dummyG1Point);
        assertEquals(SW_OK, resp.getSW());
    }

    @Test
    public void testSetSwapPubkey_setsStatusFlag() {
        send(INS_SET_SWAP_PUBKEY, dummyG1Point);

        ResponseAPDU status = send(INS_GET_STATUS);
        byte flags = status.getData()[0];
        assertNotEquals("Swap key flag should be set", 0, flags & 0x04);
    }

    @Test
    public void testSetSwapPubkey_wrongLength() {
        byte[] tooShort = new byte[32];
        ResponseAPDU resp = send(INS_SET_SWAP_PUBKEY, tooShort);
        assertEquals(SW_WRONG_LENGTH, resp.getSW());
    }

    // ========================================================================
    // CLA / INS rejection tests
    // ========================================================================

    @Test
    public void testWrongCla_rejected() {
        CommandAPDU cmd = new CommandAPDU(0x00, INS_GET_STATUS, 0x00, 0x00, 256);
        ResponseAPDU resp = simulator.transmitCommand(cmd);
        assertEquals(SW_CLA_NOT_SUPPORTED, resp.getSW());
    }

    @Test
    public void testUnknownIns_rejected() {
        ResponseAPDU resp = send((byte) 0xFF);
        assertEquals(SW_INS_NOT_SUPPORTED, resp.getSW());
    }

    @Test
    public void testBadCurveVersion_rejected() {
        // Most commands (except RESET_BLOOM and GET_STATUS) should reject BLS12-381
        generateKey();

        ResponseAPDU resp = send(INS_PUBLIC_KEY_SHARE, VERSION_BLS381, dummyG1Point);
        assertEquals(SW_BAD_VERSION, resp.getSW());
    }

    // ========================================================================
    // Full commit/respond flow tests
    // ========================================================================

    @Test
    public void testFullSignFlow_noBasename() {
        generateKey();

        // Commit
        ResponseAPDU commit = send(INS_SIGN_COMMIT, dummyG1Point);
        assertEquals(SW_OK, commit.getSW());
        assertEquals(G1_BYTES, commit.getData().length);

        // Status should show active session
        ResponseAPDU status = send(INS_GET_STATUS);
        assertNotEquals("Session should be active", 0, status.getData()[0] & 0x02);

        // Respond
        ResponseAPDU respond = send(INS_SIGN_RESPOND, dummyScalar);
        assertEquals(SW_OK, respond.getSW());
        assertEquals(FR_BYTES, respond.getData().length);

        // Session should be cleared
        status = send(INS_GET_STATUS);
        assertEquals("Session should be cleared", 0, status.getData()[0] & 0x02);
    }

    @Test
    public void testFullSignFlow_withBasename() {
        generateKey();

        byte[] input = new byte[G1_BYTES * 2];
        System.arraycopy(dummyG1Point, 0, input, 0, G1_BYTES);
        System.arraycopy(dummyG1Point, 0, input, G1_BYTES, G1_BYTES);

        // Commit
        ResponseAPDU commit = send(INS_SIGN_COMMIT_BSN, input);
        assertEquals(SW_OK, commit.getSW());
        assertEquals("Should return U_card + K_card + K_u_card",
                     G1_BYTES * 3, commit.getData().length);

        // Respond
        ResponseAPDU respond = send(INS_SIGN_RESPOND, dummyScalar);
        assertEquals(SW_OK, respond.getSW());
        assertEquals(FR_BYTES, respond.getData().length);
    }

    @Test
    public void testFullJoinFlow() {
        generateKey();

        // Commit
        ResponseAPDU commit = send(INS_JOIN_COMMIT, dummyG1Point);
        assertEquals(SW_OK, commit.getSW());
        assertEquals(G1_BYTES, commit.getData().length);

        // Respond
        ResponseAPDU respond = send(INS_JOIN_RESPOND, dummyScalar);
        assertEquals(SW_OK, respond.getSW());
        assertEquals(FR_BYTES, respond.getData().length);
    }

    @Test
    public void testMultipleSignSessions() {
        generateKey();

        // First sign session
        send(INS_SIGN_COMMIT, dummyG1Point);
        ResponseAPDU resp1 = send(INS_SIGN_RESPOND, dummyScalar);
        assertEquals(SW_OK, resp1.getSW());

        // Second sign session with fresh commit
        send(INS_SIGN_COMMIT, dummyG1Point);
        ResponseAPDU resp2 = send(INS_SIGN_RESPOND, dummyScalar);
        assertEquals(SW_OK, resp2.getSW());
    }

    // ========================================================================
    // EC math correctness tests (real crypto, not stubs)
    // ========================================================================

    @Test
    public void testPublicKeyShare_producesValidPoint() {
        generateKey();

        // Use the BN254 generator as base point
        ResponseAPDU resp = send(INS_PUBLIC_KEY_SHARE, BN254Params.G1_UNCOMPRESSED);
        assertEquals(SW_OK, resp.getSW());

        byte[] q = resp.getData();
        assertEquals(G1_BYTES, q.length);
        assertEquals("Should have uncompressed prefix", 0x04, q[0] & 0xFF);

        // Q_card = G * card_sk should NOT be the identity or the generator
        // (overwhelmingly unlikely for a random card_sk)
        boolean allZero = true;
        for (int i = 1; i < G1_BYTES; i++) {
            if (q[i] != 0) { allZero = false; break; }
        }
        assertFalse("Q_card should not be identity point", allZero);
    }

    @Test
    public void testPublicKeyShare_deterministic() {
        generateKey();

        // Same base point should produce same Q_card (card_sk is fixed)
        ResponseAPDU resp1 = send(INS_PUBLIC_KEY_SHARE, BN254Params.G1_UNCOMPRESSED);
        ResponseAPDU resp2 = send(INS_PUBLIC_KEY_SHARE, BN254Params.G1_UNCOMPRESSED);

        assertArrayEquals("Same base should give same Q_card",
                          resp1.getData(), resp2.getData());
    }

    @Test
    public void testSignRespond_producesNonTrivialScalar() {
        generateKey();

        send(INS_SIGN_COMMIT, dummyG1Point);
        ResponseAPDU resp = send(INS_SIGN_RESPOND, dummyScalar);
        assertEquals(SW_OK, resp.getSW());

        // s_card = r_card + c * card_sk should not be all zeros
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

        // Each commit generates a new random r_card, so U_card should differ
        ResponseAPDU commit1 = send(INS_SIGN_COMMIT, dummyG1Point);
        // Need to complete the session before starting a new one
        send(INS_SIGN_RESPOND, dummyScalar);

        ResponseAPDU commit2 = send(INS_SIGN_COMMIT, dummyG1Point);

        // Different r_card -> different U_card (with overwhelming probability)
        // This could theoretically fail if both r_card values happen to be equal,
        // but that's a 1/2^256 chance.
        assertFalse("Different sessions should produce different U_card",
                    java.util.Arrays.equals(commit1.getData(), commit2.getData()));
    }

    @Test
    public void testCommitOverwritesPreviousSession() {
        generateKey();

        // Start a sign session
        send(INS_SIGN_COMMIT, dummyG1Point);

        // Start another sign session (overwrites the first)
        send(INS_SIGN_COMMIT, dummyG1Point);

        // Respond should still work (with the second session's r_card)
        ResponseAPDU resp = send(INS_SIGN_RESPOND, dummyScalar);
        assertEquals(SW_OK, resp.getSW());
    }
}
