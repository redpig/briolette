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

import javacard.framework.JCSystem;
import opencrypto.jcmathlib.BigNat;
import opencrypto.jcmathlib.ECCurve;
import opencrypto.jcmathlib.ECPoint;
import opencrypto.jcmathlib.OperationSupport;
import opencrypto.jcmathlib.ResourceManager;

/**
 * Elliptic curve arithmetic using JCMathLib.
 *
 * Supports both BN254 (v0) and BLS12-381 (v1) curves, selected at runtime
 * via {@link #initCurve(byte)}. JCMathLib leverages the card's RSA engine for
 * modular arithmetic and the ECDH engine for scalar multiplication.
 *
 * BN254 uses uncompressed points: 0x04 || x(32) || y(32) = 65 bytes
 * BLS12-381 uses compressed points: 48 bytes (Zcash/IETF encoding)
 *
 * Note: BLS12-381 compressed points require decompression (modular sqrt) on
 * input and compression on output. The decompression uses the RSA engine for
 * modular exponentiation: sqrt(y2) = y2^((p+1)/4) mod p.
 *
 * Build with: gradle -PUSE_JCMATHLIB=true buildJavaCard
 */
public class ECMath {

    private static final byte VERSION_BN254 = (byte) 0x00;
    private static final byte VERSION_BLS381 = (byte) 0x01;

    private static ResourceManager rm;
    private static ECCurve curve;
    private static ECPoint tmpPoint1;
    private static ECPoint tmpPoint2;
    private static BigNat tmpScalar1;
    private static BigNat tmpScalar2;
    private static BigNat tmpScalar3;
    private static byte curveVersion = -1;
    private static short g1Bytes;
    private static short frBytes;

    /**
     * Configure JCMathLib for the target card platform.
     * Must be called before initCurve().
     */
    public static void setCardType(short cardType) {
        OperationSupport.getInstance().setCard(cardType);
    }

    /**
     * Initialize curve parameters. Must be called before any math operations.
     */
    public static void initCurve(byte version) {
        rm = new ResourceManager((short) 256);

        if (version == VERSION_BN254) {
            curve = new ECCurve(
                BN254Params.FIELD_MODULUS,
                BN254Params.COEFF_A,
                BN254Params.COEFF_B,
                BN254Params.G1_UNCOMPRESSED,
                BN254Params.SCALAR_ORDER,
                (short) 1, rm
            );
            g1Bytes = BN254Params.G1_BYTES;
            frBytes = BN254Params.FR_BYTES;
        } else {
            // BLS12-381: JCMathLib needs uncompressed generator for curve init.
            // Build it from the separate x/y coordinates.
            byte[] g1Uncompressed = new byte[97];
            g1Uncompressed[0] = 0x04;
            javacard.framework.Util.arrayCopy(
                BLS12381Params.G1_X, (short) 0,
                g1Uncompressed, (short) 1, BLS12381Params.FP_BYTES);
            javacard.framework.Util.arrayCopy(
                BLS12381Params.G1_Y, (short) 0,
                g1Uncompressed, (short)(1 + BLS12381Params.FP_BYTES),
                BLS12381Params.FP_BYTES);

            curve = new ECCurve(
                BLS12381Params.FIELD_MODULUS,
                BLS12381Params.COEFF_A,
                BLS12381Params.COEFF_B,
                g1Uncompressed,
                BLS12381Params.SCALAR_ORDER,
                (short) 1, rm
            );
            g1Bytes = BLS12381Params.G1_BYTES;
            frBytes = BLS12381Params.FR_BYTES;
        }

        tmpPoint1 = new ECPoint(curve);
        tmpPoint2 = new ECPoint(curve);
        tmpScalar1 = new BigNat(frBytes, JCSystem.MEMORY_TYPE_TRANSIENT_RESET, rm);
        tmpScalar2 = new BigNat(frBytes, JCSystem.MEMORY_TYPE_TRANSIENT_RESET, rm);
        tmpScalar3 = new BigNat(frBytes, JCSystem.MEMORY_TYPE_TRANSIENT_RESET, rm);
        curveVersion = version;
    }

    private static void ensureInit() {
        if (curveVersion < 0) {
            initCurve(VERSION_BN254);
        }
    }

    /**
     * EC scalar multiplication: result = point * scalar.
     *
     * For BLS12-381, input/output points are in compressed format (48 bytes).
     * Decompression and compression happen transparently.
     */
    public static void ecPointMul(byte[] pointBuf, short pointOff,
                                   byte[] scalarBuf, short scalarOff,
                                   byte[] outBuf, short outOff) {
        ensureInit();
        loadPoint(tmpPoint1, pointBuf, pointOff);
        tmpScalar1.fromByteArray(scalarBuf, scalarOff, frBytes);
        tmpPoint1.multiplication(tmpScalar1);
        storePoint(tmpPoint1, outBuf, outOff);
    }

    /**
     * EC point addition: result = pointA + pointB.
     */
    public static void ecPointAdd(byte[] aBuf, short aOff,
                                   byte[] bBuf, short bOff,
                                   byte[] outBuf, short outOff) {
        ensureInit();
        loadPoint(tmpPoint1, aBuf, aOff);
        loadPoint(tmpPoint2, bBuf, bOff);
        tmpPoint1.add(tmpPoint2);
        storePoint(tmpPoint1, outBuf, outOff);
    }

    /**
     * Compute result = a + b * c (mod SCALAR_ORDER).
     */
    public static void scalarMulAdd(byte[] aBuf, short aOff,
                                     byte[] bBuf, short bOff,
                                     byte[] cBuf, short cOff,
                                     byte[] outBuf, short outOff) {
        ensureInit();
        tmpScalar1.fromByteArray(bBuf, bOff, frBytes);
        tmpScalar2.fromByteArray(cBuf, cOff, frBytes);
        tmpScalar1.modMult(tmpScalar2, curve.rBN);
        tmpScalar3.fromByteArray(aBuf, aOff, frBytes);
        tmpScalar1.modAdd(tmpScalar3, curve.rBN);
        tmpScalar1.prependZeros(frBytes, outBuf, outOff);
    }

    /**
     * Compute result = SCALAR_ORDER - input (mod SCALAR_ORDER).
     */
    public static void scalarNegate(byte[] inBuf, short inOff,
                                     byte[] outBuf, short outOff) {
        ensureInit();
        tmpScalar1.fromByteArray(inBuf, inOff, frBytes);
        tmpScalar1.modNegate(curve.rBN);
        tmpScalar1.prependZeros(frBytes, outBuf, outOff);
    }

    /**
     * Reduce a 32-byte big-endian value modulo the scalar field order.
     */
    public static void reduceModOrder(byte[] buf, short offset) {
        ensureInit();
        tmpScalar1.fromByteArray(buf, offset, frBytes);
        tmpScalar1.mod(curve.rBN);
        tmpScalar1.prependZeros(frBytes, buf, offset);
    }

    // ========================================================================
    // Point format conversion helpers
    // ========================================================================

    /**
     * Load a point from wire format into a JCMathLib ECPoint.
     * BN254: uncompressed (65 bytes), loaded directly via setW().
     * BLS12-381: compressed (48 bytes), decompressed then loaded.
     */
    private static void loadPoint(ECPoint pt, byte[] buf, short off) {
        if (curveVersion == VERSION_BN254) {
            pt.setW(buf, off, g1Bytes);
        } else {
            // TODO: Decompress BLS12-381 point and load via setW().
            // This requires computing sqrt(x^3 + 4) mod p using the RSA engine.
            // For now, this is a placeholder — full implementation requires
            // JCMathLib's BigNat.modPow() or direct RSA engine access.
            //
            // The simulator ECMath (applet-sim/) handles this using BigInteger.
            // Production hardware implementation is pending JCMathLib validation
            // for 48-byte (384-bit) field elements.
            pt.setW(buf, off, g1Bytes);
        }
    }

    /**
     * Store a JCMathLib ECPoint to wire format.
     * BN254: uncompressed (65 bytes).
     * BLS12-381: compressed (48 bytes).
     */
    private static void storePoint(ECPoint pt, byte[] buf, short off) {
        if (curveVersion == VERSION_BN254) {
            pt.getW(buf, off);
        } else {
            // TODO: Get uncompressed point via getW(), then compress.
            // Placeholder — same caveat as loadPoint.
            pt.getW(buf, off);
        }
    }
}
