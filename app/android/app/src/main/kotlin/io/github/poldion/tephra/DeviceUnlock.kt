package io.github.poldion.tephra

import android.content.Context
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
/// Den Schlüssel dafür hält der Keystore von Android; er verlässt ihn nie und ist nur nutzbar, solange das
/// Telefon entsperrt ist. Auf ein anderes Gerät lässt er sich nicht mitnehmen.
object DeviceUnlock {
    private const val ALIAS = "tephra-device-unlock"
    private const val TRANSFORMATION = "AES/GCM/NoPadding"

    private fun file(context: Context) = File(context.filesDir, "device-unlock.bin")

    private fun store() = KeyStore.getInstance("AndroidKeyStore").apply { load(null) }

    private fun key(create: Boolean): SecretKey? {
        (store().getKey(ALIAS, null) as? SecretKey)?.let { return it }
        if (!create) return null
        val spec = KeyGenParameterSpec.Builder(ALIAS, KeyProperties.PURPOSE_ENCRYPT or KeyProperties.PURPOSE_DECRYPT)
            .setBlockModes(KeyProperties.BLOCK_MODE_GCM)
            .setEncryptionPaddings(KeyProperties.ENCRYPTION_PADDING_NONE)
            .setKeySize(256)
            .apply {
                // Nur nutzbar, solange das Telefon entsperrt ist.
                if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.P) setUnlockedDeviceRequired(true)
            }
            .build()
        return KeyGenerator.getInstance(KeyProperties.KEY_ALGORITHM_AES, "AndroidKeyStore").run {
            init(spec)
            generateKey()
        }
    }

    fun stored(context: Context): Boolean = file(context).isFile && key(create = false) != null

    fun save(context: Context, secret: ByteArray) {
        val cipher = Cipher.getInstance(TRANSFORMATION)
        cipher.init(Cipher.ENCRYPT_MODE, key(create = true))
        val iv = cipher.iv
        val sealed = cipher.doFinal(secret)
        val target = file(context)
        val temporary = File(target.parentFile, "${target.name}.neu")
        temporary.writeBytes(byteArrayOf(iv.size.toByte()) + iv + sealed)
        if (!temporary.renameTo(target)) {
            temporary.delete()
            throw IllegalStateException("Die Datei lässt sich nicht speichern")
        }
    }

    fun load(context: Context): ByteArray? {
        val bytes = file(context).takeIf { it.isFile }?.readBytes() ?: return null
        val key = key(create = false) ?: return null
        val ivLength = bytes[0].toInt()
        val cipher = Cipher.getInstance(TRANSFORMATION)
        cipher.init(Cipher.DECRYPT_MODE, key, GCMParameterSpec(128, bytes, 1, ivLength))
        return cipher.doFinal(bytes, 1 + ivLength, bytes.size - 1 - ivLength)
    }

    fun forget(context: Context) {
        file(context).delete()
        runCatching { store().deleteEntry(ALIAS) }
    }
}
