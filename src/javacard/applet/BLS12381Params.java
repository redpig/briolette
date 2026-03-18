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

/**
 * BLS12-381 curve parameters for G1 operations.
 *
 * BLS12-381 is a pairing-friendly curve providing 128-bit security, used in
 * Briolette's v1 ECDAA protocol. The smart card only needs G1 scalar
 * multiplication and Fr (scalar field) arithmetic.
 *
 * Curve equation: y^2 = x^3 + 4 over GF(p)
 *
 * Wire format uses compressed G1 points (48 bytes) matching the Zcash/IETF
 * encoding where the high bits of the first byte encode flags:
 *   bit 7: compressed flag (always 1)
 *   bit 6: infinity flag
 *   bit 5: sign of y (lexicographically larger = 1)
 *
 * These constants match the bls12_381_plus Rust crate used in src/crypto/src/v1.rs.
 */
public class BLS12381Params {
    /** Size of a scalar field element (Fr) in bytes. Same as BN254. */
    public static final short FR_BYTES = 32;

    /** Size of a G1 point in compressed form (48 bytes). */
    public static final short G1_BYTES = 48;

    /** Size of a field element (Fp) coordinate in bytes. */
    public static final short FP_BYTES = 48;

    /**
     * Field modulus p (381 bits).
     * p = 0x1a0111ea397fe69a4b1ba7b6434bacd764774b84f38512bf
     *       6730d2a0f6b0f6241eabfffeb153ffffb9feffffffffaaab
     */
    public static final byte[] FIELD_MODULUS = {
        (byte)0x1a, (byte)0x01, (byte)0x11, (byte)0xea,
        (byte)0x39, (byte)0x7f, (byte)0xe6, (byte)0x9a,
        (byte)0x4b, (byte)0x1b, (byte)0xa7, (byte)0xb6,
        (byte)0x43, (byte)0x4b, (byte)0xac, (byte)0xd7,
        (byte)0x64, (byte)0x77, (byte)0x4b, (byte)0x84,
        (byte)0xf3, (byte)0x85, (byte)0x12, (byte)0xbf,
        (byte)0x67, (byte)0x30, (byte)0xd2, (byte)0xa0,
        (byte)0xf6, (byte)0xb0, (byte)0xf6, (byte)0x24,
        (byte)0x1e, (byte)0xab, (byte)0xff, (byte)0xfe,
        (byte)0xb1, (byte)0x53, (byte)0xff, (byte)0xff,
        (byte)0xb9, (byte)0xfe, (byte)0xff, (byte)0xff,
        (byte)0xff, (byte)0xff, (byte)0xaa, (byte)0xab
    };

    /**
     * Scalar field order r (255 bits).
     * r = 0x73eda753299d7d483339d80809a1d80553bda402fffe5bfeffffffff00000001
     */
    public static final byte[] SCALAR_ORDER = {
        (byte)0x73, (byte)0xed, (byte)0xa7, (byte)0x53,
        (byte)0x29, (byte)0x9d, (byte)0x7d, (byte)0x48,
        (byte)0x33, (byte)0x39, (byte)0xd8, (byte)0x08,
        (byte)0x09, (byte)0xa1, (byte)0xd8, (byte)0x05,
        (byte)0x53, (byte)0xbd, (byte)0xa4, (byte)0x02,
        (byte)0xff, (byte)0xfe, (byte)0x5b, (byte)0xfe,
        (byte)0xff, (byte)0xff, (byte)0xff, (byte)0xff,
        (byte)0x00, (byte)0x00, (byte)0x00, (byte)0x01
    };

    /**
     * Generator point G1 x-coordinate (48 bytes).
     */
    public static final byte[] G1_X = {
        (byte)0x17, (byte)0xf1, (byte)0xd3, (byte)0xa7,
        (byte)0x31, (byte)0x97, (byte)0xd7, (byte)0x94,
        (byte)0x26, (byte)0x95, (byte)0x63, (byte)0x8c,
        (byte)0x4f, (byte)0xa9, (byte)0xac, (byte)0x0f,
        (byte)0xc3, (byte)0x68, (byte)0x8c, (byte)0x4f,
        (byte)0x97, (byte)0x74, (byte)0xb9, (byte)0x05,
        (byte)0xa1, (byte)0x4e, (byte)0x3a, (byte)0x3f,
        (byte)0x17, (byte)0x1b, (byte)0xac, (byte)0x58,
        (byte)0x6c, (byte)0x55, (byte)0xe8, (byte)0x3f,
        (byte)0xf9, (byte)0x7a, (byte)0x1a, (byte)0xef,
        (byte)0xfb, (byte)0x3a, (byte)0xf0, (byte)0x0a,
        (byte)0xdb, (byte)0x22, (byte)0xc6, (byte)0xbb
    };

    /**
     * Generator point G1 y-coordinate (48 bytes).
     */
    public static final byte[] G1_Y = {
        (byte)0x08, (byte)0xb3, (byte)0xf4, (byte)0x81,
        (byte)0xe3, (byte)0xaa, (byte)0xa0, (byte)0xf1,
        (byte)0xa0, (byte)0x9e, (byte)0x30, (byte)0xed,
        (byte)0x74, (byte)0x1d, (byte)0x8a, (byte)0xe4,
        (byte)0xfc, (byte)0xf5, (byte)0xe0, (byte)0x95,
        (byte)0xd5, (byte)0xd0, (byte)0x0a, (byte)0xf6,
        (byte)0x00, (byte)0xdb, (byte)0x18, (byte)0xcb,
        (byte)0x2c, (byte)0x04, (byte)0xb3, (byte)0xed,
        (byte)0xd0, (byte)0x3c, (byte)0xc7, (byte)0x44,
        (byte)0xa2, (byte)0x88, (byte)0x8a, (byte)0xe4,
        (byte)0x0c, (byte)0xaa, (byte)0x23, (byte)0x29,
        (byte)0x46, (byte)0xc5, (byte)0xe7, (byte)0xe1
    };

    /**
     * Curve coefficient a = 0 (48 bytes, zero-padded).
     */
    public static final byte[] COEFF_A = new byte[48];

    /**
     * Curve coefficient b = 4.
     */
    public static final byte[] COEFF_B = {
        (byte)0x00, (byte)0x00, (byte)0x00, (byte)0x00,
        (byte)0x00, (byte)0x00, (byte)0x00, (byte)0x00,
        (byte)0x00, (byte)0x00, (byte)0x00, (byte)0x00,
        (byte)0x00, (byte)0x00, (byte)0x00, (byte)0x00,
        (byte)0x00, (byte)0x00, (byte)0x00, (byte)0x00,
        (byte)0x00, (byte)0x00, (byte)0x00, (byte)0x00,
        (byte)0x00, (byte)0x00, (byte)0x00, (byte)0x00,
        (byte)0x00, (byte)0x00, (byte)0x00, (byte)0x00,
        (byte)0x00, (byte)0x00, (byte)0x00, (byte)0x00,
        (byte)0x00, (byte)0x00, (byte)0x00, (byte)0x00,
        (byte)0x00, (byte)0x00, (byte)0x00, (byte)0x00,
        (byte)0x00, (byte)0x00, (byte)0x00, (byte)0x04
    };

    /**
     * Generator point G1 in compressed form (48 bytes).
     * This is the standard BLS12-381 compressed encoding with the
     * compression flag set (bit 7 of first byte).
     */
    public static final byte[] G1_COMPRESSED;

    static {
        // Build compressed generator: set compression bit on G1_X,
        // then set sign bit if y is lexicographically larger than -y.
        G1_COMPRESSED = new byte[48];
        System.arraycopy(G1_X, 0, G1_COMPRESSED, 0, 48);
        // Set compression flag (bit 7)
        G1_COMPRESSED[0] |= (byte) 0x80;
        // The generator's y-coordinate (0x08b3...) is the smaller of the two
        // possible y values (the other being p - y), so the sign bit (bit 5)
        // is NOT set. The Zcash convention: bit 5 = 1 if y is the
        // lexicographically larger root. For the standard generator, y < p/2,
        // so bit 5 = 0.
    }
}
