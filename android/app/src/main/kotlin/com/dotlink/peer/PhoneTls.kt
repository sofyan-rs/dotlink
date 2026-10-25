package com.dotlink.peer

import android.security.keystore.KeyGenParameterSpec
import android.security.keystore.KeyProperties
import com.dotlink.link.toHex
import java.security.KeyPairGenerator
import java.security.KeyStore
import java.security.MessageDigest
import java.security.cert.X509Certificate
import java.security.spec.ECGenParameterSpec
import java.util.Date
import javax.net.ssl.KeyManagerFactory
import javax.net.ssl.SSLContext
import javax.security.auth.x500.X500Principal

/**
 * This phone's TLS identity for other phones: an EC key in the Android Keystore with the
 * self-signed certificate the Keystore makes for it. Other phones pin the certificate's SHA-256
 * at pairing, like the phone pins the PC's.
 */
object PhoneTls {
    private const val ALIAS = "dotlink-phone-tls"

    private fun keyStore(): KeyStore = KeyStore.getInstance("AndroidKeyStore").apply { load(null) }

    @Synchronized
    private fun ensureKey(store: KeyStore) {
        if (store.containsAlias(ALIAS)) return
        val spec =
            KeyGenParameterSpec
                .Builder(ALIAS, KeyProperties.PURPOSE_SIGN)
                .setAlgorithmParameterSpec(ECGenParameterSpec("secp256r1"))
                // TLS signs pre-hashed data on some paths (NONE) and hashes itself on others.
                .setDigests(KeyProperties.DIGEST_NONE, KeyProperties.DIGEST_SHA256, KeyProperties.DIGEST_SHA384, KeyProperties.DIGEST_SHA512)
                .setCertificateSubject(X500Principal("CN=dotlink"))
                .setCertificateNotBefore(Date(0))
                .setCertificateNotAfter(Date(System.currentTimeMillis() + 30L * 365 * 24 * 3600 * 1000))
                .build()
        KeyPairGenerator.getInstance(KeyProperties.KEY_ALGORITHM_EC, "AndroidKeyStore").apply { initialize(spec) }.generateKeyPair()
    }

    private val certificate: X509Certificate by lazy {
        val store = keyStore()
        ensureKey(store)
        store.getCertificate(ALIAS) as X509Certificate
    }

    /** Lowercase hex SHA-256 of the certificate, what other phones pin. */
    val fingerprint: String by lazy { MessageDigest.getInstance("SHA-256").digest(certificate.encoded).toHex() }

    /** For the server socket: presents this certificate, signs with the Keystore key. */
    fun serverContext(): SSLContext {
        val store = keyStore()
        ensureKey(store)
        val kmf = KeyManagerFactory.getInstance(KeyManagerFactory.getDefaultAlgorithm()).apply { init(store, null) }
        return SSLContext.getInstance("TLS").apply { init(kmf.keyManagers, null, null) }
    }
}
