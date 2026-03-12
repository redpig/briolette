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
 * Elliptic curve arithmetic on BN254 using JCMathLib.
 *
 * This is the production implementation for real JavaCard hardware. It uses
 * JCMathLib's ECPoint and BigNat classes to perform EC scalar multiplication,
 * point addition, and modular scalar arithmetic on BN254 — operations not
 * natively supported by the JavaCard crypto API for non-standard curves.
 *
 * JCMathLib leverages the card's RSA engine for modular arithmetic and the
 * ECDH engine for scalar multiplication, so no proprietary APIs are needed.
 *
 * Build with: gradle -PUSE_JCMATHLIB=true buildJavaCard
 * Or set JCMATHLIB_HOME to the JCMathLib JAR directory.
 *
 * For simulator testing without JCMathLib, the build system selects the
 * BigInteger-based applet-sim/ECMath.java instead (same API, pure Java math).
 */
public class ECMath {

    // Lazily initialized JCMathLib objects.
    // These are allocated once and reused across APDU calls.
    private static ResourceManager rm;
    private static ECCurve curve;
    private static ECPoint tmpPoint1;
    private static ECPoint tmpPoint2;
    private static BigNat tmpScalar1;
    private static BigNat tmpScalar2;
    private static BigNat tmpScalar3;
    private static boolean initialized = false;

    /**
     * Initialize JCMathLib on first use.
     *
     * Call setCardType() before the first math operation to configure
     * JCMathLib for the target hardware. Defaults to SIMULATOR for
     * jCardSim testing.
     */
    private static void ensureInit() {
        if (initialized) {
            return;
        }

        rm = new ResourceManager((short) 256);
        curve = new ECCurve(
            BN254Params.FIELD_MODULUS,
            BN254Params.COEFF_A,
            BN254Params.COEFF_B,
            BN254Params.G1_UNCOMPRESSED,
            BN254Params.SCALAR_ORDER,
            (short) 1,  // cofactor
            rm
        );
        tmpPoint1 = new ECPoint(curve);
        tmpPoint2 = new ECPoint(curve);
        tmpScalar1 = new BigNat(
            (short) BN254Params.FR_BYTES,
            JCSystem.MEMORY_TYPE_TRANSIENT_RESET, rm);
        tmpScalar2 = new BigNat(
            (short) BN254Params.FR_BYTES,
            JCSystem.MEMORY_TYPE_TRANSIENT_RESET, rm);
        tmpScalar3 = new BigNat(
            (short) BN254Params.FR_BYTES,
            JCSystem.MEMORY_TYPE_TRANSIENT_RESET, rm);
        initialized = true;
    }

    /**
     * Configure JCMathLib for the target card platform.
     * Must be called before the first math operation (e.g., during install).
     *
     * @param cardType One of OperationSupport.SIMULATOR, JCOP4_P71, etc.
     */
    public static void setCardType(short cardType) {
        OperationSupport.getInstance().setCard(cardType);
    }

    /**
     * EC scalar multiplication: result = point * scalar.
     */
    public static void ecPointMul(byte[] pointBuf, short pointOff,
                                   byte[] scalarBuf, short scalarOff,
                                   byte[] outBuf, short outOff) {
        ensureInit();
        tmpPoint1.setW(pointBuf, pointOff, BN254Params.G1_BYTES);
        tmpScalar1.fromByteArray(scalarBuf, scalarOff, BN254Params.FR_BYTES);
        tmpPoint1.multiplication(tmpScalar1);
        tmpPoint1.getW(outBuf, outOff);
    }

    /**
     * EC point addition: result = pointA + pointB.
     */
    public static void ecPointAdd(byte[] aBuf, short aOff,
                                   byte[] bBuf, short bOff,
                                   byte[] outBuf, short outOff) {
        ensureInit();
        tmpPoint1.setW(aBuf, aOff, BN254Params.G1_BYTES);
        tmpPoint2.setW(bBuf, bOff, BN254Params.G1_BYTES);
        tmpPoint1.add(tmpPoint2);
        tmpPoint1.getW(outBuf, outOff);
    }

    /**
     * Compute result = a + b * c (mod SCALAR_ORDER).
     */
    public static void scalarMulAdd(byte[] aBuf, short aOff,
                                     byte[] bBuf, short bOff,
                                     byte[] cBuf, short cOff,
                                     byte[] outBuf, short outOff) {
        ensureInit();
        // tmp1 = b
        tmpScalar1.fromByteArray(bBuf, bOff, BN254Params.FR_BYTES);
        // tmp2 = c
        tmpScalar2.fromByteArray(cBuf, cOff, BN254Params.FR_BYTES);
        // tmp1 = b * c mod order
        tmpScalar1.modMult(tmpScalar2, curve.rBN);
        // tmp3 = a
        tmpScalar3.fromByteArray(aBuf, aOff, BN254Params.FR_BYTES);
        // tmp1 = (b * c) + a mod order
        tmpScalar1.modAdd(tmpScalar3, curve.rBN);
        // Write result
        tmpScalar1.prependZeros(BN254Params.FR_BYTES, outBuf, outOff);
    }

    /**
     * Compute result = SCALAR_ORDER - input (mod SCALAR_ORDER).
     */
    public static void scalarNegate(byte[] inBuf, short inOff,
                                     byte[] outBuf, short outOff) {
        ensureInit();
        tmpScalar1.fromByteArray(inBuf, inOff, BN254Params.FR_BYTES);
        tmpScalar1.modNegate(curve.rBN);
        tmpScalar1.prependZeros(BN254Params.FR_BYTES, outBuf, outOff);
    }

    /**
     * Reduce a 32-byte big-endian value modulo the scalar field order.
     */
    public static void reduceModOrder(byte[] buf, short offset) {
        ensureInit();
        tmpScalar1.fromByteArray(buf, offset, BN254Params.FR_BYTES);
        tmpScalar1.mod(curve.rBN);
        tmpScalar1.prependZeros(BN254Params.FR_BYTES, buf, offset);
    }
}
