package dev.jcode.mobile.ui

import android.content.Context
import androidx.compose.runtime.*
import androidx.compose.ui.platform.LocalContext
import dev.jcode.mobile.data.*

internal class ThreadReadStore(context: Context) {
    private val preferences = context.applicationContext.getSharedPreferences("thread_read_evidence_v1", Context.MODE_PRIVATE)
    private val ledger = CatchUpLedger(preferences.getString("seen", "").orEmpty())
    private val activity = linkedMapOf<String, Pair<String, Boolean>>().apply {
        preferences.getString("activity", "").orEmpty().lineSequence().take(64).forEach { line ->
            val parts = line.split(':')
            if (parts.size == 3 && parts.take(2).all { it.matches(Regex("[a-f0-9]{64}")) }) put(parts[0], parts[1] to (parts[2] == "1"))
        }
    }
    var revision by mutableIntStateOf(0)
        private set
    fun observeActivity(host: String, sessions: List<MobileSession>) {
        var changed = false
        sessions.take(64).forEach { session ->
            val key = catchUpSessionKey(host, session.id)
            val snapshot = catchUpActivityKey(session)
            val previous = activity[key]
            if (previous?.first != snapshot) {
                activity.remove(key)
                activity[key] = snapshot to (previous != null)
                changed = true
            }
        }
        while (activity.size > 64) activity.remove(activity.keys.first())
        if (changed) saveActivity()
    }
    fun hasActivity(host: String, sessionId: String): Boolean {
        @Suppress("UNUSED_VARIABLE") val version = revision
        return activity[catchUpSessionKey(host, sessionId)]?.second == true
    }
    fun viewLatestActivity(host: String, sessionId: String) {
        val key = catchUpSessionKey(host, sessionId)
        val current = activity[key] ?: return
        if (current.second) { activity[key] = current.first to false; saveActivity() }
    }
    private fun saveActivity() {
        revision++
        preferences.edit().putString("activity", activity.entries.joinToString("\n") { (key, value) -> "$key:${value.first}:${if (value.second) 1 else 0}" }).apply()
    }
    fun unseen(host: String, sessionId: String, entry: TranscriptEntry): Boolean {
        @Suppress("UNUSED_VARIABLE") val version = revision
        return catchUpEligible(entry) && !ledger.hasSeen(catchUpSessionKey(host, sessionId), catchUpMessageKey(entry))
    }
    fun acknowledge(host: String, sessionId: String, entries: List<TranscriptEntry>) {
        if (ledger.acknowledge(catchUpSessionKey(host, sessionId), entries.filter(::catchUpEligible).map(::catchUpMessageKey))) {
            revision++
            preferences.edit().putString("seen", ledger.encode()).apply()
        }
    }
}
internal val LocalThreadReadingAllowed = staticCompositionLocalOf { true }
internal val LocalThreadReadStore = staticCompositionLocalOf<ThreadReadStore?> { null }
@Composable
internal fun rememberThreadReadStore(): ThreadReadStore {
    val context = LocalContext.current
    val local = LocalThreadReadStore.current
    return local ?: remember(context.applicationContext) { ThreadReadStore(context) }
}
internal fun MobileState.readScope(): String = if (isDemo) "demo" else host
