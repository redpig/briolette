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

import java.math.BigInteger;

/**
 * Elliptic curve arithmetic on BN254 using java.math.BigInteger.
 *
 * This class provides the actual EC math operations needed by BrioletteApplet:
 * G1 scalar multiplication, point addition, and Fr scalar arithmetic.
 *
 * It uses affine coordinates over GF(p) with the standard addition formulas.
 * Point-at-infinity is represented as (0, 0) with a special flag.
 *
 * NOTE: This implementation uses java.math.BigInteger, which is available in
 * jCardSim but NOT on real JavaCard hardware. For production deployment on
 * physical smart cards, replace these operations with JCMathLib calls:
 *
 *   ECMath.ecPointMul(...)    -> ECPoint.multiplication(BigNat)
 *   ECMath.ecPointAdd(...)    -> ECPoint.add(ECPoint)
 *   ECMath.scalarMulAdd(...)  -> BigNat.modMult() + BigNat.modAdd()
 *   ECMath.scalarNegate(...)  -> BigNat.subtract()
 *   ECMath.reduceModOrder(...) -> BigNat.mod()
 *
 * See the JCMathLib initialization pattern in BrioletteApplet constructor
 * comments for the production integration path:
 *   ResourceManager rm = new ResourceManager((short) 256);
 *   ECCurve curve = new ECCurve(p, a, b, G, r, k, rm);
 *   ECPoint pt = new ECPoint(curve);
 *   BigNat scalar = new BigNat(curve.rBN.length(), MEMORY_TYPE, rm);
 */
public class ECMath {

    /** BN254 field modulus p. */
    private static final BigInteger P = new BigInteger(1, BN254Params.FIELD_MODULUS);

    /** BN254 scalar field order r. */
    private static final BigInteger R = new BigInteger(1, BN254Params.SCALAR_ORDER);

    /** Curve coefficient b = 3. */
    private static final BigInteger B = BigInteger.valueOf(3);

    /** Curve coefficient a = 0. */
    private static final BigInteger A = BigInteger.ZERO;

    /**
     * EC scalar multiplication: result = point * scalar.
     *
     * Uses double-and-add algorithm in affine coordinates.
     *
     * @param pointBuf  Buffer containing uncompressed G1 point (0x04 || x || y)
     * @param pointOff  Offset of point in buffer
     * @param scalarBuf Buffer containing 32-byte big-endian scalar
     * @param scalarOff Offset of scalar in buffer
     * @param outBuf    Output buffer for result point
     * @param outOff    Offset in output buffer
     */
    public static void ecPointMul(byte[] pointBuf, short pointOff,
                                   byte[] scalarBuf, short scalarOff,
                                   byte[] outBuf, short outOff) {
        BigInteger x = readCoord(pointBuf, (short)(pointOff + 1));
        BigInteger y = readCoord(pointBuf, (short)(pointOff + 33));
        BigInteger k = readScalar(scalarBuf, scalarOff);

        // Handle identity cases
        if (k.signum() == 0) {
            writeIdentity(outBuf, outOff);
            return;
        }

        // Double-and-add
        BigInteger[] result = null; // point at infinity
        BigInteger[] base = new BigInteger[]{x, y};

        for (int i = k.bitLength() - 1; i >= 0; i--) {
            if (result != null) {
                result = pointDouble(result[0], result[1]);
            }
            if (k.testBit(i)) {
                if (result == null) {
                    result = new BigInteger[]{base[0], base[1]};
                } else {
                    result = pointAdd(result[0], result[1], base[0], base[1]);
                }
            }
        }

        if (result == null) {
            writeIdentity(outBuf, outOff);
        } else {
            writePoint(outBuf, outOff, result[0], result[1]);
        }
    }

    /**
     * EC point addition: result = pointA + pointB.
     *
     * @param aBuf   Buffer containing first uncompressed G1 point
     * @param aOff   Offset of first point
     * @param bBuf   Buffer containing second uncompressed G1 point
     * @param bOff   Offset of second point
     * @param outBuf Output buffer for result point
     * @param outOff Offset in output buffer
     */
    public static void ecPointAdd(byte[] aBuf, short aOff,
                                   byte[] bBuf, short bOff,
                                   byte[] outBuf, short outOff) {
        BigInteger ax = readCoord(aBuf, (short)(aOff + 1));
        BigInteger ay = readCoord(aBuf, (short)(aOff + 33));
        BigInteger bx = readCoord(bBuf, (short)(bOff + 1));
        BigInteger by = readCoord(bBuf, (short)(bOff + 33));

        // Handle identity
        if (isIdentity(ax, ay)) {
            writePoint(outBuf, outOff, bx, by);
            return;
        }
        if (isIdentity(bx, by)) {
            writePoint(outBuf, outOff, ax, ay);
            return;
        }

        BigInteger[] result = pointAdd(ax, ay, bx, by);
        if (result == null) {
            writeIdentity(outBuf, outOff);
        } else {
            writePoint(outBuf, outOff, result[0], result[1]);
        }
    }

    /**
     * Compute result = a + b * c (mod SCALAR_ORDER).
     * Core operation for Schnorr responses: s_card = r_card + challenge * card_sk.
     */
    public static void scalarMulAdd(byte[] aBuf, short aOff,
                                     byte[] bBuf, short bOff,
                                     byte[] cBuf, short cOff,
                                     byte[] outBuf, short outOff) {
        BigInteger a = readScalar(aBuf, aOff);
        BigInteger b = readScalar(bBuf, bOff);
        BigInteger c = readScalar(cBuf, cOff);

        BigInteger result = b.multiply(c).mod(R).add(a).mod(R);
        writeScalar(outBuf, outOff, result);
    }

    /**
     * Compute result = SCALAR_ORDER - input (mod SCALAR_ORDER).
     * Negation in the scalar field.
     */
    public static void scalarNegate(byte[] inBuf, short inOff,
                                     byte[] outBuf, short outOff) {
        BigInteger val = readScalar(inBuf, inOff);
        BigInteger result = R.subtract(val).mod(R);
        writeScalar(outBuf, outOff, result);
    }

    /**
     * Reduce a 32-byte big-endian value modulo the scalar field order.
     */
    public static void reduceModOrder(byte[] buf, short offset) {
        BigInteger val = readScalar(buf, offset);
        BigInteger result = val.mod(R);
        writeScalar(buf, offset, result);
    }

    // ========================================================================
    // Affine EC arithmetic helpers
    // ========================================================================

    private static boolean isIdentity(BigInteger x, BigInteger y) {
        return x.signum() == 0 && y.signum() == 0;
    }

    /**
     * Affine point addition P + Q.
     * Returns null for point-at-infinity.
     */
    private static BigInteger[] pointAdd(BigInteger x1, BigInteger y1,
                                          BigInteger x2, BigInteger y2) {
        if (x1.equals(x2)) {
            if (y1.equals(y2)) {
                return pointDouble(x1, y1);
            }
            // P + (-P) = O
            return null;
        }

        // lambda = (y2 - y1) / (x2 - x1) mod p
        BigInteger dx = x2.subtract(x1).mod(P);
        BigInteger dy = y2.subtract(y1).mod(P);
        BigInteger lambda = dy.multiply(dx.modInverse(P)).mod(P);

        // x3 = lambda^2 - x1 - x2 mod p
        BigInteger x3 = lambda.multiply(lambda).subtract(x1).subtract(x2).mod(P);
        // y3 = lambda * (x1 - x3) - y1 mod p
        BigInteger y3 = lambda.multiply(x1.subtract(x3)).subtract(y1).mod(P);

        return new BigInteger[]{x3, y3};
    }

    /**
     * Affine point doubling 2P.
     * Returns null for point-at-infinity.
     */
    private static BigInteger[] pointDouble(BigInteger x, BigInteger y) {
        if (y.signum() == 0) {
            return null;
        }

        // lambda = (3*x^2 + a) / (2*y) mod p   (a = 0 for BN254)
        BigInteger num = x.multiply(x).mod(P).multiply(BigInteger.valueOf(3)).add(A).mod(P);
        BigInteger den = y.multiply(BigInteger.valueOf(2)).mod(P);
        BigInteger lambda = num.multiply(den.modInverse(P)).mod(P);

        // x3 = lambda^2 - 2*x mod p
        BigInteger x3 = lambda.multiply(lambda).subtract(x.multiply(BigInteger.valueOf(2))).mod(P);
        // y3 = lambda * (x - x3) - y mod p
        BigInteger y3 = lambda.multiply(x.subtract(x3)).subtract(y).mod(P);

        return new BigInteger[]{x3, y3};
    }

    // ========================================================================
    // Serialization helpers
    // ========================================================================

    /** Read a 32-byte big-endian unsigned integer from a buffer. */
    private static BigInteger readCoord(byte[] buf, short off) {
        byte[] tmp = new byte[32];
        System.arraycopy(buf, off, tmp, 0, 32);
        return new BigInteger(1, tmp);
    }

    /** Read a 32-byte big-endian scalar from a buffer. */
    private static BigInteger readScalar(byte[] buf, short off) {
        byte[] tmp = new byte[32];
        System.arraycopy(buf, off, tmp, 0, 32);
        return new BigInteger(1, tmp);
    }

    /** Write a BigInteger as a 32-byte big-endian value, zero-padded. */
    private static void writeScalar(byte[] buf, short off, BigInteger val) {
        byte[] encoded = val.toByteArray();
        // BigInteger.toByteArray() may have a leading zero byte for sign
        int srcOff = 0;
        int len = encoded.length;
        if (len > 32 && encoded[0] == 0) {
            srcOff = 1;
            len--;
        }
        // Zero-fill the output
        for (int i = 0; i < 32; i++) {
            buf[off + i] = 0;
        }
        // Copy right-aligned
        int destOff = 32 - len;
        if (destOff < 0) {
            srcOff -= destOff;
            len += destOff;
            destOff = 0;
        }
        System.arraycopy(encoded, srcOff, buf, off + destOff, len);
    }

    /** Write an EC point in uncompressed form (0x04 || x || y). */
    private static void writePoint(byte[] buf, short off,
                                    BigInteger x, BigInteger y) {
        buf[off] = 0x04;
        writeScalar(buf, (short)(off + 1), x);
        writeScalar(buf, (short)(off + 33), y);
    }

    /** Write the point-at-infinity as all zeros with 0x04 prefix. */
    private static void writeIdentity(byte[] buf, short off) {
        buf[off] = 0x04;
        for (int i = 1; i < 65; i++) {
            buf[off + i] = 0;
        }
    }
}
