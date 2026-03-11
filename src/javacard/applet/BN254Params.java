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
 * BN254 curve parameters for G1 operations.
 *
 * BN254 (alt_bn128) is a Barreto-Naehrig pairing-friendly curve used in
 * Briolette's v0 ECDAA protocol. The smart card only needs G1 operations
 * and Fr (scalar field) arithmetic.
 *
 * Curve equation: y^2 = x^3 + 3 over GF(p)
 *
 * These constants match the bn254 crate used in the Rust implementation
 * (the "BN254" variant used in Ethereum's alt_bn128 precompiles).
 */
public class BN254Params {
    /** Size of a scalar field element (Fr) in bytes. */
    public static final short FR_BYTES = 32;

    /** Size of a G1 point in uncompressed form (0x04 || x || y). */
    public static final short G1_BYTES = 65;

    /** Uncompressed point prefix byte. */
    public static final byte UNCOMPRESSED_PREFIX = 0x04;

    /**
     * Field modulus p (base field GF(p)).
     * p = 21888242871839275222246405745257275088696311157297823662689037894645226208583
     */
    public static final byte[] FIELD_MODULUS = {
        (byte)0x30, (byte)0x64, (byte)0x4e, (byte)0x72,
        (byte)0xe1, (byte)0x31, (byte)0xa0, (byte)0x29,
        (byte)0xb8, (byte)0x50, (byte)0x45, (byte)0xb6,
        (byte)0x81, (byte)0x81, (byte)0x58, (byte)0x5d,
        (byte)0x97, (byte)0x81, (byte)0x6a, (byte)0x91,
        (byte)0x68, (byte)0x71, (byte)0xca, (byte)0x8d,
        (byte)0x3c, (byte)0x20, (byte)0x8c, (byte)0x16,
        (byte)0xd8, (byte)0x7c, (byte)0xfd, (byte)0x47
    };

    /**
     * Scalar field order r (Fr).
     * r = 21888242871839275222246405745257275088548364400416034343698204186575808495617
     */
    public static final byte[] SCALAR_ORDER = {
        (byte)0x30, (byte)0x64, (byte)0x4e, (byte)0x72,
        (byte)0xe1, (byte)0x31, (byte)0xa0, (byte)0x29,
        (byte)0xb8, (byte)0x50, (byte)0x45, (byte)0xb6,
        (byte)0x81, (byte)0x81, (byte)0x58, (byte)0x5d,
        (byte)0x28, (byte)0x33, (byte)0xe8, (byte)0x48,
        (byte)0x79, (byte)0xb9, (byte)0x70, (byte)0x91,
        (byte)0x43, (byte)0xe1, (byte)0xf5, (byte)0x93,
        (byte)0xf0, (byte)0x00, (byte)0x00, (byte)0x01
    };

    /**
     * Generator point G1 x-coordinate.
     * G1.x = 1
     */
    public static final byte[] G1_X = {
        (byte)0x00, (byte)0x00, (byte)0x00, (byte)0x00,
        (byte)0x00, (byte)0x00, (byte)0x00, (byte)0x00,
        (byte)0x00, (byte)0x00, (byte)0x00, (byte)0x00,
        (byte)0x00, (byte)0x00, (byte)0x00, (byte)0x00,
        (byte)0x00, (byte)0x00, (byte)0x00, (byte)0x00,
        (byte)0x00, (byte)0x00, (byte)0x00, (byte)0x00,
        (byte)0x00, (byte)0x00, (byte)0x00, (byte)0x00,
        (byte)0x00, (byte)0x00, (byte)0x00, (byte)0x01
    };

    /**
     * Generator point G1 y-coordinate.
     * G1.y = 2
     */
    public static final byte[] G1_Y = {
        (byte)0x00, (byte)0x00, (byte)0x00, (byte)0x00,
        (byte)0x00, (byte)0x00, (byte)0x00, (byte)0x00,
        (byte)0x00, (byte)0x00, (byte)0x00, (byte)0x00,
        (byte)0x00, (byte)0x00, (byte)0x00, (byte)0x00,
        (byte)0x00, (byte)0x00, (byte)0x00, (byte)0x00,
        (byte)0x00, (byte)0x00, (byte)0x00, (byte)0x00,
        (byte)0x00, (byte)0x00, (byte)0x00, (byte)0x00,
        (byte)0x00, (byte)0x00, (byte)0x00, (byte)0x02
    };

    /**
     * Curve coefficient a = 0.
     */
    public static final byte[] COEFF_A = {
        (byte)0x00, (byte)0x00, (byte)0x00, (byte)0x00,
        (byte)0x00, (byte)0x00, (byte)0x00, (byte)0x00,
        (byte)0x00, (byte)0x00, (byte)0x00, (byte)0x00,
        (byte)0x00, (byte)0x00, (byte)0x00, (byte)0x00,
        (byte)0x00, (byte)0x00, (byte)0x00, (byte)0x00,
        (byte)0x00, (byte)0x00, (byte)0x00, (byte)0x00,
        (byte)0x00, (byte)0x00, (byte)0x00, (byte)0x00,
        (byte)0x00, (byte)0x00, (byte)0x00, (byte)0x00
    };

    /**
     * Curve coefficient b = 3.
     */
    public static final byte[] COEFF_B = {
        (byte)0x00, (byte)0x00, (byte)0x00, (byte)0x00,
        (byte)0x00, (byte)0x00, (byte)0x00, (byte)0x00,
        (byte)0x00, (byte)0x00, (byte)0x00, (byte)0x00,
        (byte)0x00, (byte)0x00, (byte)0x00, (byte)0x00,
        (byte)0x00, (byte)0x00, (byte)0x00, (byte)0x00,
        (byte)0x00, (byte)0x00, (byte)0x00, (byte)0x00,
        (byte)0x00, (byte)0x00, (byte)0x00, (byte)0x00,
        (byte)0x00, (byte)0x00, (byte)0x00, (byte)0x03
    };
}
