package dev.jcode.mobile.data

import android.content.Context
import android.security.keystore.KeyGenParameterSpec
import android.security.keystore.KeyProperties
import android.util.Base64
import org.json.JSONObject
import java.security.KeyStore
import javax.crypto.Cipher
import javax.crypto.KeyGenerator
import javax.crypto.SecretKey
import javax.crypto.spec.GCMParameterSpec

internal data class Credential(val host: String, val token: String, val serverName: String)

/** Only AES-GCM ciphertext leaves Android Keystore. No credentials in logs or backup. */
internal class CredentialStore(context: Context) {
    private val prefs by lazy { context.getSharedPreferences("gateway_credentials", Context.MODE_PRIVATE) }
    private val alias = "dev.jcode.mobile.gateway.v1"
    private fun key(): SecretKey {
        val store = KeyStore.getInstance("AndroidKeyStore").apply { load(null) }
        (store.getKey(alias, null) as? SecretKey)?.let { return it }
        return KeyGenerator.getInstance(KeyProperties.KEY_ALGORITHM_AES, "AndroidKeyStore").apply {
            init(KeyGenParameterSpec.Builder(alias, KeyProperties.PURPOSE_ENCRYPT or KeyProperties.PURPOSE_DECRYPT)
                .setBlockModes(KeyProperties.BLOCK_MODE_GCM).setEncryptionPaddings(KeyProperties.ENCRYPTION_PADDING_NONE)
                .setRandomizedEncryptionRequired(true).build())
        }.generateKey()
    }
    fun save(value: Credential) {
        val cipher = Cipher.getInstance("AES/GCM/NoPadding").apply { init(Cipher.ENCRYPT_MODE, key()) }
        val clear = JSONObject().put("host", value.host).put("token", value.token).put("name", value.serverName).toString().toByteArray()
        val encrypted = cipher.doFinal(clear)
        check(prefs.edit().putString("iv", Base64.encodeToString(cipher.iv, Base64.NO_WRAP))
            .putString("data", Base64.encodeToString(encrypted, Base64.NO_WRAP)).commit()) { "Could not save pairing securely" }
    }
    fun load(): Credential? {
        val data = prefs.getString("data", null) ?: return null
        val iv = prefs.getString("iv", null) ?: error("Saved pairing is incomplete. Pair again.")
        val cipher = Cipher.getInstance("AES/GCM/NoPadding").apply {
            init(Cipher.DECRYPT_MODE, key(), GCMParameterSpec(128, Base64.decode(iv, Base64.NO_WRAP)))
        }
        val json = JSONObject(String(cipher.doFinal(Base64.decode(data, Base64.NO_WRAP))))
        return Credential(json.getString("host"), json.getString("token"), json.getString("name"))
    }
    fun clear() { check(prefs.edit().clear().commit()) { "Could not remove saved pairing" } }
}
