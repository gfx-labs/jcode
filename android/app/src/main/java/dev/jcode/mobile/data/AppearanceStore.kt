package dev.jcode.mobile.data

import android.content.Context
import org.json.JSONObject

enum class ThemeMode { SYSTEM, LIGHT, DARK }
data class SessionCustomization(val name: String = "", val pinned: Boolean = false)
data class AppearancePreferences(val theme: ThemeMode = ThemeMode.SYSTEM, val sessions: Map<String, SessionCustomization> = emptyMap()) {
    private fun key(host: String, id: String) = JSONObject().put("host", host.trim().trimEnd('/')).put("id", id).toString()
    fun session(host: String, id: String) = sessions[key(host, id)] ?: SessionCustomization()
    fun update(host: String, id: String, value: SessionCustomization): AppearancePreferences {
        val key = key(host, id)
        return copy(sessions = if (value == SessionCustomization()) sessions - key else sessions + (key to value))
    }
    fun present(state: MobileState): MobileState = state.copy(sessions = state.sessions.map {
        val custom = session(if (state.isDemo) "demo" else state.host, it.id)
        it.copy(name = custom.name.ifBlank { it.name }, pinned = custom.pinned)
    })
}

/** UI preferences contain no credentials. Session overrides are scoped to gateway and ID. */
class AppearanceStore(context: Context) {
    private val prefs = context.getSharedPreferences("appearance", Context.MODE_PRIVATE)
    fun load(): AppearancePreferences {
        val theme = runCatching { ThemeMode.valueOf(prefs.getString("theme", "SYSTEM")!!) }.getOrDefault(ThemeMode.SYSTEM)
        val entries = runCatching {
            val json = JSONObject(prefs.getString("sessions", "{}")!!)
            json.keys().asSequence().associateWith { key ->
                val item = json.getJSONObject(key)
                SessionCustomization(item.optString("name"), item.optBoolean("pinned"))
            }
        }.getOrDefault(emptyMap())
        return AppearancePreferences(theme, entries)
    }
    fun save(value: AppearancePreferences) {
        val json = JSONObject()
        value.sessions.forEach { (key, item) -> json.put(key, JSONObject().put("name", item.name).put("pinned", item.pinned)) }
        prefs.edit().putString("theme", value.theme.name).putString("sessions", json.toString()).apply()
    }
}
