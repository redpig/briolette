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
 * Elliptic curve arithmetic using java.math.BigInteger.
 *
 * Supports both BN254 (v0) and BLS12-381 (v1) curves, selected at runtime
 * via {@link #initCurve(byte)}. The active curve determines field modulus,
 * scalar order, point serialization format, and coordinate sizes.
 *
 * BN254 uses uncompressed points: 0x04 || x(32) || y(32) = 65 bytes
 * BLS12-381 uses compressed points: 48 bytes (Zcash/IETF encoding)
 *
 * NOTE: This implementation uses java.math.BigInteger, which is available in
 * jCardSim but NOT on real JavaCard hardware. For production deployment on
 * physical smart cards, replace with JCMathLib (see applet-hw/ECMath.java).
 */
public class ECMath {

    private static final byte VERSION_BN254 = (byte) 0x00;
    private static final byte VERSION_BLS381 = (byte) 0x01;

    /** Active curve parameters. */
    private static BigInteger P;
    private static BigInteger R;
    private static BigInteger A;
    private static BigInteger B_COEFF;
    private static short frBytes;
    private static short g1Bytes;
    private static short coordBytes;
    private static byte curveVersion = -1;

    /**
     * Initialize curve parameters. Must be called before any math operations.
     *
     * @param version VERSION_BN254 (0x00) or VERSION_BLS381 (0x01)
     */
    public static void initCurve(byte version) {
        if (version == VERSION_BN254) {
            P = new BigInteger(1, BN254Params.FIELD_MODULUS);
            R = new BigInteger(1, BN254Params.SCALAR_ORDER);
            A = BigInteger.ZERO;
            B_COEFF = BigInteger.valueOf(3);
            frBytes = BN254Params.FR_BYTES;
            g1Bytes = BN254Params.G1_BYTES;
            coordBytes = 32;
        } else {
            P = new BigInteger(1, BLS12381Params.FIELD_MODULUS);
            R = new BigInteger(1, BLS12381Params.SCALAR_ORDER);
            A = BigInteger.ZERO;
            B_COEFF = BigInteger.valueOf(4);
            frBytes = BLS12381Params.FR_BYTES;
            g1Bytes = BLS12381Params.G1_BYTES;
            coordBytes = 48;
        }
        curveVersion = version;
    }

    /** Ensure a curve has been initialized. Defaults to BN254 for backwards compat. */
    private static void ensureInit() {
        if (curveVersion < 0) {
            initCurve(VERSION_BN254);
        }
    }

    /**
     * EC scalar multiplication: result = point * scalar.
     * Uses double-and-add algorithm in affine coordinates.
     */
    public static void ecPointMul(byte[] pointBuf, short pointOff,
                                   byte[] scalarBuf, short scalarOff,
                                   byte[] outBuf, short outOff) {
        ensureInit();
        BigInteger[] pt = readPoint(pointBuf, pointOff);
        BigInteger k = readScalar(scalarBuf, scalarOff);

        if (k.signum() == 0 || pt == null) {
            writeIdentity(outBuf, outOff);
            return;
        }

        BigInteger[] result = null;
        BigInteger[] base = pt;

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
     */
    public static void ecPointAdd(byte[] aBuf, short aOff,
                                   byte[] bBuf, short bOff,
                                   byte[] outBuf, short outOff) {
        ensureInit();
        BigInteger[] ptA = readPoint(aBuf, aOff);
        BigInteger[] ptB = readPoint(bBuf, bOff);

        if (ptA == null) {
            if (ptB == null) {
                writeIdentity(outBuf, outOff);
            } else {
                writePoint(outBuf, outOff, ptB[0], ptB[1]);
            }
            return;
        }
        if (ptB == null) {
            writePoint(outBuf, outOff, ptA[0], ptA[1]);
            return;
        }

        BigInteger[] result = pointAdd(ptA[0], ptA[1], ptB[0], ptB[1]);
        if (result == null) {
            writeIdentity(outBuf, outOff);
        } else {
            writePoint(outBuf, outOff, result[0], result[1]);
        }
    }

    /**
     * Compute result = a + b * c (mod SCALAR_ORDER).
     */
    public static void scalarMulAdd(byte[] aBuf, short aOff,
                                     byte[] bBuf, short bOff,
                                     byte[] cBuf, short cOff,
                                     byte[] outBuf, short outOff) {
        ensureInit();
        BigInteger a = readScalar(aBuf, aOff);
        BigInteger b = readScalar(bBuf, bOff);
        BigInteger c = readScalar(cBuf, cOff);

        BigInteger result = b.multiply(c).mod(R).add(a).mod(R);
        writeScalar(outBuf, outOff, result);
    }

    /**
     * Compute result = SCALAR_ORDER - input (mod SCALAR_ORDER).
     */
    public static void scalarNegate(byte[] inBuf, short inOff,
                                     byte[] outBuf, short outOff) {
        ensureInit();
        BigInteger val = readScalar(inBuf, inOff);
        BigInteger result = R.subtract(val).mod(R);
        writeScalar(outBuf, outOff, result);
    }

    /**
     * Reduce a 32-byte big-endian value modulo the scalar field order.
     */
    public static void reduceModOrder(byte[] buf, short offset) {
        ensureInit();
        BigInteger val = readScalar(buf, offset);
        BigInteger result = val.mod(R);
        writeScalar(buf, offset, result);
    }

    // ========================================================================
    // Affine EC arithmetic helpers
    // ========================================================================

    private static BigInteger[] pointAdd(BigInteger x1, BigInteger y1,
                                          BigInteger x2, BigInteger y2) {
        if (x1.equals(x2)) {
            if (y1.equals(y2)) {
                return pointDouble(x1, y1);
            }
            return null;
        }

        BigInteger dx = x2.subtract(x1).mod(P);
        BigInteger dy = y2.subtract(y1).mod(P);
        BigInteger lambda = dy.multiply(dx.modInverse(P)).mod(P);

        BigInteger x3 = lambda.multiply(lambda).subtract(x1).subtract(x2).mod(P);
        BigInteger y3 = lambda.multiply(x1.subtract(x3)).subtract(y1).mod(P);

        return new BigInteger[]{x3, y3};
    }

    private static BigInteger[] pointDouble(BigInteger x, BigInteger y) {
        if (y.signum() == 0) {
            return null;
        }

        BigInteger num = x.multiply(x).mod(P).multiply(BigInteger.valueOf(3)).add(A).mod(P);
        BigInteger den = y.multiply(BigInteger.valueOf(2)).mod(P);
        BigInteger lambda = num.multiply(den.modInverse(P)).mod(P);

        BigInteger x3 = lambda.multiply(lambda).subtract(x.multiply(BigInteger.valueOf(2))).mod(P);
        BigInteger y3 = lambda.multiply(x.subtract(x3)).subtract(y).mod(P);

        return new BigInteger[]{x3, y3};
    }

    // ========================================================================
    // Point serialization (format depends on active curve)
    // ========================================================================

    /**
     * Read a G1 point from a buffer.
     * BN254: uncompressed (0x04 || x(32) || y(32)) = 65 bytes
     * BLS12-381: compressed (48 bytes, Zcash format)
     *
     * @return [x, y] or null for identity
     */
    private static BigInteger[] readPoint(byte[] buf, short off) {
        if (curveVersion == VERSION_BN254) {
            return readPointUncompressed(buf, off);
        } else {
            return readPointCompressed(buf, off);
        }
    }

    private static BigInteger[] readPointUncompressed(byte[] buf, short off) {
        BigInteger x = readCoord(buf, (short)(off + 1), (short) 32);
        BigInteger y = readCoord(buf, (short)(off + 33), (short) 32);
        if (x.signum() == 0 && y.signum() == 0) {
            return null; // identity
        }
        return new BigInteger[]{x, y};
    }

    /**
     * Read a BLS12-381 compressed G1 point (48 bytes, Zcash/IETF format).
     * High bits of first byte: bit7=compressed, bit6=infinity, bit5=y_sign.
     */
    private static BigInteger[] readPointCompressed(byte[] buf, short off) {
        byte flags = buf[off];
        boolean isInfinity = (flags & 0x40) != 0;
        if (isInfinity) {
            return null;
        }
        boolean ySign = (flags & 0x20) != 0;

        // Extract x: clear the flag bits from the first byte
        byte[] xBytes = new byte[48];
        System.arraycopy(buf, off, xBytes, 0, 48);
        xBytes[0] &= 0x1F; // clear top 3 flag bits

        BigInteger x = new BigInteger(1, xBytes);

        // Recover y: y^2 = x^3 + b (mod p)
        BigInteger y2 = x.modPow(BigInteger.valueOf(3), P).add(B_COEFF).mod(P);

        // sqrt(y2) = y2^((p+1)/4) mod p  (valid since p ≡ 3 mod 4 for BLS12-381)
        BigInteger exp = P.add(BigInteger.ONE).shiftRight(2);
        BigInteger y = y2.modPow(exp, P);

        // Verify the sqrt is correct
        if (!y.multiply(y).mod(P).equals(y2)) {
            return null; // not a valid point
        }

        // Select the correct y based on the sign bit.
        // Zcash convention: bit5=1 means y is the lexicographically larger root.
        // The "larger" root is the one where y > p/2.
        BigInteger pHalf = P.shiftRight(1);
        boolean yIsLarger = y.compareTo(pHalf) > 0;
        if (ySign != yIsLarger) {
            y = P.subtract(y);
        }

        return new BigInteger[]{x, y};
    }

    /**
     * Write a G1 point to a buffer in the active curve's format.
     */
    private static void writePoint(byte[] buf, short off,
                                    BigInteger x, BigInteger y) {
        if (curveVersion == VERSION_BN254) {
            writePointUncompressed(buf, off, x, y);
        } else {
            writePointCompressed(buf, off, x, y);
        }
    }

    private static void writePointUncompressed(byte[] buf, short off,
                                                BigInteger x, BigInteger y) {
        buf[off] = 0x04;
        writeCoord(buf, (short)(off + 1), x, (short) 32);
        writeCoord(buf, (short)(off + 33), y, (short) 32);
    }

    /**
     * Write a BLS12-381 G1 point in compressed form (48 bytes).
     */
    private static void writePointCompressed(byte[] buf, short off,
                                              BigInteger x, BigInteger y) {
        writeCoord(buf, off, x, (short) 48);
        // Set compression flag (bit 7)
        buf[off] |= (byte) 0x80;
        // Set sign bit (bit 5) if y is the lexicographically larger root
        BigInteger pHalf = P.shiftRight(1);
        if (y.compareTo(pHalf) > 0) {
            buf[off] |= (byte) 0x20;
        }
    }

    /** Write the identity point. */
    private static void writeIdentity(byte[] buf, short off) {
        if (curveVersion == VERSION_BN254) {
            buf[off] = 0x04;
            for (int i = 1; i < 65; i++) {
                buf[off + i] = 0;
            }
        } else {
            // BLS12-381 compressed identity: compression flag + infinity flag
            for (int i = 0; i < 48; i++) {
                buf[off + i] = 0;
            }
            buf[off] = (byte) 0xC0; // bits 7 and 6 set
        }
    }

    // ========================================================================
    // Coordinate and scalar serialization
    // ========================================================================

    private static BigInteger readCoord(byte[] buf, short off, short len) {
        byte[] tmp = new byte[len];
        System.arraycopy(buf, off, tmp, 0, len);
        return new BigInteger(1, tmp);
    }

    private static BigInteger readScalar(byte[] buf, short off) {
        byte[] tmp = new byte[32];
        System.arraycopy(buf, off, tmp, 0, 32);
        return new BigInteger(1, tmp);
    }

    private static void writeScalar(byte[] buf, short off, BigInteger val) {
        writeCoord(buf, off, val, (short) 32);
    }

    /** Write a BigInteger as a big-endian value, zero-padded to len bytes. */
    private static void writeCoord(byte[] buf, short off, BigInteger val, short len) {
        byte[] encoded = val.toByteArray();
        int srcOff = 0;
        int encLen = encoded.length;
        if (encLen > len && encoded[0] == 0) {
            srcOff = 1;
            encLen--;
        }
        // Zero-fill
        for (int i = 0; i < len; i++) {
            buf[off + i] = 0;
        }
        // Copy right-aligned
        int destOff = len - encLen;
        if (destOff < 0) {
            srcOff -= destOff;
            encLen += destOff;
            destOff = 0;
        }
        System.arraycopy(encoded, srcOff, buf, off + destOff, encLen);
    }
}
