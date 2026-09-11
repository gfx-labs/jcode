package dev.jcode.mobile.notifications

import android.content.Context
import android.content.SharedPreferences
import dev.jcode.mobile.data.GatewayAddress
import org.json.JSONObject
import java.security.MessageDigest

/** Canonical gateway identity only. Neither credentials nor display names are stored here. */
fun notificationHostKey(host: String): String = MessageDigest.getInstance("SHA-256")
    .digest(GatewayAddress.parse(host).toString().toByteArray(Charsets.UTF_8))
    .joinToString("") { "%02x".format(it) }

data class SessionNotificationPolicy(val enabled: Boolean = false, val snoozedUntil: Long = 0) {
    fun allows(now: Long) = enabled && now >= snoozedUntil
}

class SessionNotificationStore(context: Context) {
    private val prefs = context.applicationContext.getSharedPreferences("session_notifications_v1", Context.MODE_PRIVATE)
    private fun key(hostKey: String, sessionId: String) = "$hostKey:${android.net.Uri.encode(sessionId)}"
    fun get(hostKey: String, sessionId: String): SessionNotificationPolicy = runCatching {
        val json = JSONObject(prefs.getString(key(hostKey, sessionId), null) ?: return SessionNotificationPolicy())
        SessionNotificationPolicy(json.optBoolean("enabled"), json.optLong("snoozedUntil"))
    }.getOrDefault(SessionNotificationPolicy())
    fun set(hostKey: String, sessionId: String, policy: SessionNotificationPolicy) {
        require(hostKey.matches(Regex("[a-f0-9]{64}")) && sessionId.isNotBlank())
        require(!policy.enabled || get(hostKey, sessionId).enabled || enabledSessionIds(hostKey).size < 128) {
            "Mute another session first (128 monitored sessions per gateway maximum)"
        }
        check(prefs.edit().putString(key(hostKey, sessionId), JSONObject()
            .put("enabled", policy.enabled).put("snoozedUntil", policy.snoozedUntil).toString()).commit()) {
            "Could not save notification preference"
        }
    }
    fun enabledSessionIds(hostKey: String): Set<String> = prefs.all.keys.asSequence()
        .filter { it.startsWith("$hostKey:") }
        .map { android.net.Uri.decode(it.substringAfter(':')) }
        .filter { get(hostKey, it).enabled }.toSet()
    fun listen(onChange: () -> Unit): () -> Unit {
        val listener = SharedPreferences.OnSharedPreferenceChangeListener { _, _ -> onChange() }
        prefs.registerOnSharedPreferenceChangeListener(listener)
        return { prefs.unregisterOnSharedPreferenceChangeListener(listener) }
    }
}
