package io.github.poldion.tephra

import android.content.Context
import android.hardware.biometrics.BiometricManager
import android.os.Build
import android.security.keystore.KeyGenParameterSpec
import android.security.keystore.KeyProperties
import java.io.File
import java.security.KeyStore
import javax.crypto.Cipher
import javax.crypto.KeyGenerator
import javax.crypto.SecretKey
import javax.crypto.spec.GCMParameterSpec

/// Entsperren ohne Master-Passwort: Der Schlüssel des Tresors liegt verschlüsselt im privaten Ordner der App.
/// Den Schlüssel dafür hält der Keystore von Android; er verlässt ihn nie und lässt sich nicht auf ein anderes
/// Gerät mitnehmen.
///
/// Ab Android 9 gibt der Keystore ihn nur nach einem Fingerabdruck frei (bei jeder Benutzung, nur starke
/// Biometrie, kein Ausweichen auf die PIN). Kommt am Telefon ein Fingerabdruck dazu oder fällt einer weg, macht
/// Android den Schlüssel unbrauchbar; dann braucht es wieder das Master-Passwort.
///
/// Darunter, und für Ablagen aus Version 0.8.0, gilt die alte Form: nutzbar, solange das Telefon entsperrt ist.
object DeviceUnlock {
    private const val ALIAS = "tephra-device-unlock"
    private const val ALIAS_FINGER = "tephra-device-unlock-finger"
    private const val TRANSFORMATION = "AES/GCM/NoPadding"

    /// Ob dieses Android den Fingerabdruck-Dialog des Systems kennt.
    val fingerprintSupported: Boolean get() = Build.VERSION.SDK_INT >= Build.VERSION_CODES.P

    private fun file(context: Context) = File(context.filesDir, "device-unlock.bin")
    private fun fingerFile(context: Context) = File(context.filesDir, "device-unlock-finger.bin")

    private fun store() = KeyStore.getInstance("AndroidKeyStore").apply { load(null) }

    private fun storedKey(alias: String): SecretKey? = store().getKey(alias, null) as? SecretKey

    private fun generate(alias: String, finger: Boolean): SecretKey {
        val spec = KeyGenParameterSpec.Builder(alias, KeyProperties.PURPOSE_ENCRYPT or KeyProperties.PURPOSE_DECRYPT)
            .setBlockModes(KeyProperties.BLOCK_MODE_GCM)
            .setEncryptionPaddings(KeyProperties.ENCRYPTION_PADDING_NONE)
            .setKeySize(256)
            .apply {
                // Nur nutzbar, solange das Telefon entsperrt ist.
                if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.P) setUnlockedDeviceRequired(true)
                if (finger) {
                    setUserAuthenticationRequired(true)
                    setInvalidatedByBiometricEnrollment(true)
                    if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.R) {
                        setUserAuthenticationParameters(0, KeyProperties.AUTH_BIOMETRIC_STRONG)
                    }
                    // Bis Android 10 heißt „ohne Gültigkeitsdauer“ ebenfalls: bei jeder Benutzung per Biometrie.
                }
            }
            .build()
        return KeyGenerator.getInstance(KeyProperties.KEY_ALGORITHM_AES, "AndroidKeyStore").run {
            init(spec)
            generateKey()
        }
    }

    /// `none`, `legacy` (Ablage ohne Fingerabdruck) oder `fingerprint`.
    fun kind(context: Context): String = when {
        fingerFile(context).isFile && storedKey(ALIAS_FINGER) != null -> "fingerprint"
        file(context).isFile && storedKey(ALIAS) != null -> "legacy"
        else -> "none"
    }

    fun stored(context: Context): Boolean = kind(context) != "none"

    /// Warum der Fingerabdruck auf diesem Telefon nicht geht; `null`, wenn er geht.
    fun fingerprintProblem(context: Context): String? {
        if (!fingerprintSupported) return "unsupported"
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.R) {
            val manager = context.getSystemService(BiometricManager::class.java) ?: return "nohardware"
            return when (manager.canAuthenticate(BiometricManager.Authenticators.BIOMETRIC_STRONG)) {
                BiometricManager.BIOMETRIC_SUCCESS -> null
                BiometricManager.BIOMETRIC_ERROR_NONE_ENROLLED -> "noneenrolled"
                else -> "nohardware"
            }
        }
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.Q) {
            val manager = context.getSystemService(BiometricManager::class.java) ?: return "nohardware"
            @Suppress("DEPRECATION")
            return when (manager.canAuthenticate()) {
                BiometricManager.BIOMETRIC_SUCCESS -> null
                BiometricManager.BIOMETRIC_ERROR_NONE_ENROLLED -> "noneenrolled"
                else -> "nohardware"
            }
        }
        // Android 9: Ob ein Finger eingerichtet ist, zeigt sich beim Anlegen des Schlüssels.
        return null
    }

    /// Ablage in der alten Form, für Telefone ohne Fingerabdruck-Dialog.
    fun saveLegacy(context: Context, secret: ByteArray) {
        val cipher = Cipher.getInstance(TRANSFORMATION)
        cipher.init(Cipher.ENCRYPT_MODE, storedKey(ALIAS) ?: generate(ALIAS, finger = false))
        write(file(context), cipher, secret)
    }

    fun loadLegacy(context: Context): ByteArray? {
        val bytes = file(context).takeIf { it.isFile }?.readBytes() ?: return null
        val key = storedKey(ALIAS) ?: return null
        return open(bytes, decryptCipher(key, bytes))
    }

    /// Verschlüsselung mit einem neuen, an den Fingerabdruck gebundenen Schlüssel; nutzbar erst nach dem Dialog.
    fun fingerEncryptCipher(): Cipher {
        runCatching { store().deleteEntry(ALIAS_FINGER) }
        val cipher = Cipher.getInstance(TRANSFORMATION)
        cipher.init(Cipher.ENCRYPT_MODE, generate(ALIAS_FINGER, finger = true))
        return cipher
    }

    /// Schreibt die Ablage mit der freigegebenen Verschlüsselung und entfernt eine ältere ohne Fingerabdruck.
    fun saveFinger(context: Context, cipher: Cipher, secret: ByteArray) {
        write(fingerFile(context), cipher, secret)
        file(context).delete()
        runCatching { store().deleteEntry(ALIAS) }
    }

    /// Entschlüsselung der Ablage; nutzbar erst nach dem Dialog. Wirft `KeyPermanentlyInvalidatedException`,
    /// wenn sich die Fingerabdrücke des Telefons geändert haben.
    fun fingerDecryptCipher(context: Context): Cipher {
        val bytes = fingerFile(context).readBytes()
        val key = storedKey(ALIAS_FINGER) ?: throw IllegalStateException("Der Schlüssel fehlt im Keystore")
        return decryptCipher(key, bytes)
    }

    fun openFinger(context: Context, cipher: Cipher): ByteArray = open(fingerFile(context).readBytes(), cipher)

    fun forget(context: Context) {
        file(context).delete()
        fingerFile(context).delete()
        runCatching { store().deleteEntry(ALIAS) }
        runCatching { store().deleteEntry(ALIAS_FINGER) }
    }

    private fun write(target: File, cipher: Cipher, secret: ByteArray) {
        val iv = cipher.iv
        val sealed = cipher.doFinal(secret)
        val temporary = File(target.parentFile, "${target.name}.neu")
        temporary.writeBytes(byteArrayOf(iv.size.toByte()) + iv + sealed)
        if (!temporary.renameTo(target)) {
            temporary.delete()
            throw IllegalStateException("Die Datei lässt sich nicht speichern")
        }
    }

    private fun decryptCipher(key: SecretKey, bytes: ByteArray): Cipher {
        val ivLength = bytes[0].toInt()
        val cipher = Cipher.getInstance(TRANSFORMATION)
        cipher.init(Cipher.DECRYPT_MODE, key, GCMParameterSpec(128, bytes, 1, ivLength))
        return cipher
    }

    private fun open(bytes: ByteArray, cipher: Cipher): ByteArray {
        val ivLength = bytes[0].toInt()
        return cipher.doFinal(bytes, 1 + ivLength, bytes.size - 1 - ivLength)
    }
}
