package com.briolette.wallet

import android.os.Build
import android.security.keystore.KeyGenParameterSpec
import android.security.keystore.KeyProperties
import android.util.Base64
import com.briolette.wallet.data.HwAttestationData
import com.briolette.wallet.data.HwAttestationProvider
import java.nio.ByteBuffer
import java.security.KeyPairGenerator
import java.security.KeyStore
import java.security.cert.Certificate

/**
 * Generates Android Key Attestation data for hardware-backed registration.
 *
 * Uses the Android Keystore to generate an attested EC P-256 key. The resulting
 * cert chain (leaf → intermediate → root) proves the key was generated inside
 * TEE or StrongBox hardware. The registrar verifies this chain against Google's
 * root certificates and extracts the KeyDescription extension (OID 1.3.6.1.4.1.11129.2.1.17)
 * to confirm hardware-backed key generation.
 *
 * The [challenge] bytes are embedded in the attestation certificate's KeyDescription
 * extension as the attestationChallenge field. The registrar uses this to bind
 * the attestation to the specific registration request (hw_id).
 */
object AndroidKeyAttestation {

    private const val ALIAS = "briolette_hw_attestation"

    /**
     * Generate Key Attestation and return [HwAttestationData] ready for registration.
     *
     * @param challenge The attestation challenge (typically the hw_id bytes).
     * @return attestation data with algorithm=1 (ANDROID_KM_ATTESTATION),
     *         signatureB64=length-prefixed DER cert chain, publicKeyB64=attested public key.
     */
    fun generate(challenge: ByteArray): HwAttestationData {
        // Delete any previous key with the same alias.
        val ks = KeyStore.getInstance("AndroidKeyStore")
        ks.load(null)
        if (ks.containsAlias(ALIAS)) {
            ks.deleteEntry(ALIAS)
        }

        // Generate an attested EC key in hardware.
        val spec = KeyGenParameterSpec.Builder(ALIAS, KeyProperties.PURPOSE_SIGN)
            .setAlgorithmParameterSpec(java.security.spec.ECGenParameterSpec("secp256r1"))
            .setDigests(KeyProperties.DIGEST_SHA256)
            .setAttestationChallenge(challenge)
            .build()

        val kpg = KeyPairGenerator.getInstance(
            KeyProperties.KEY_ALGORITHM_EC,
            "AndroidKeyStore",
        )
        kpg.initialize(spec)
        val keyPair = kpg.generateKeyPair()

        // Retrieve the attestation certificate chain.
        val chain: Array<Certificate> = ks.getCertificateChain(ALIAS)
            ?: throw IllegalStateException("No attestation certificate chain available")

        // Encode the cert chain as length-prefixed DER: [u32-BE len][DER bytes]...
        val certChainBytes = encodeCertChain(chain)

        // The attested public key is the raw encoded form of the EC key.
        val pubKeyBytes = keyPair.public.encoded

        return HwAttestationData(
            algorithm = 1, // ANDROID_KM_ATTESTATION
            signatureB64 = Base64.encodeToString(certChainBytes, Base64.NO_WRAP),
            publicKeyB64 = Base64.encodeToString(pubKeyBytes, Base64.NO_WRAP),
        )
    }

    /**
     * Encode a certificate chain as length-prefixed DER.
     * Format: [u32-BE length][DER bytes][u32-BE length][DER bytes]...
     */
    private fun encodeCertChain(chain: Array<Certificate>): ByteArray {
        var totalSize = 0
        val derCerts = chain.map { it.encoded }
        for (der in derCerts) {
            totalSize += 4 + der.size
        }
        val buf = ByteBuffer.allocate(totalSize)
        for (der in derCerts) {
            buf.putInt(der.size)
            buf.put(der)
        }
        return buf.array()
    }
}

/**
 * [HwAttestationProvider] implementation using Android KeyStore Key Attestation.
 *
 * Decodes the base64 challenge preimage (hw_id || nac_pk || ttc_pk),
 * SHA-256 hashes it to get the actual attestation challenge, then generates
 * Android Key Attestation with that challenge. This cryptographically binds
 * the hardware attestation to the specific ECDAA credential public keys.
 */
class AndroidKeyAttestationProvider : HwAttestationProvider {
    override val isSupported: Boolean
        get() = Build.VERSION.SDK_INT >= Build.VERSION_CODES.N

    override suspend fun generate(challengePreimageB64: String): HwAttestationData? {
        return try {
            val preimage = Base64.decode(challengePreimageB64, Base64.NO_WRAP)
            val digest = java.security.MessageDigest.getInstance("SHA-256")
            val challenge = digest.digest(preimage)
            AndroidKeyAttestation.generate(challenge)
        } catch (e: Exception) {
            null
        }
    }
}
