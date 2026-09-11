package dev.jcode.mobile.data

import org.json.JSONObject

/** Provider-reported allowance, not session token accounting or an estimated credit balance. */
data class QuotaWindow(val name: String, val remainingPercent: Double?, val resetsAt: String?)
data class ProviderQuota(val provider: String, val name: String, val available: Boolean, val limits: List<QuotaWindow>, val error: String?)
data class ProviderUsageState(val loading: Boolean = false, val providers: List<ProviderQuota> = emptyList(), val fetchedAtSeconds: Long? = null, val fromCache: Boolean = false, val error: String? = null)

internal fun decodeProviderUsage(event: JSONObject): ProviderUsageState {
    require(event.text("type") == "mobile_usage") { "Update the Jcode daemon to view provider usage." }
    return ProviderUsageState(providers = event.objects("providers").take(32).map { provider ->
        ProviderQuota(provider.text("provider"), provider.text("display_name"), provider.optBoolean("available"),
            provider.objects("limits").take(32).map { limit ->
                val remaining = (limit.opt("remaining_percent") as? Number)?.toDouble()
                QuotaWindow(limit.text("name"), remaining?.takeIf { it.isFinite() && it in 0.0..100.0 }, limit.nullableText("resets_at"))
            }, provider.nullableText("error"))
    }, fetchedAtSeconds = (event.opt("fetched_at_unix_secs") as? Number)?.toLong()?.takeIf { it in 1..253402300799L },
        fromCache = event.optBoolean("from_cache"), error = event.nullableText("error"))
}

internal fun canonicalUsageProvider(name: String): String = when (val normalized = name.lowercase().replace('_', '-')) {
    "anthropic", "claude", "claude-api" -> "claude"
    "openai", "openai-api", "codex" -> "openai"
    "github-copilot", "copilot" -> "copilot"
    else -> normalized
}
