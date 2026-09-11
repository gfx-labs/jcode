package dev.jcode.mobile.ui

import android.content.Context
import android.content.ContextWrapper
import android.content.Intent
import android.provider.Settings
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.material3.Button
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.Switch
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.unit.dp
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import dev.jcode.mobile.MainActivity
import dev.jcode.mobile.notifications.SessionMonitorService
import dev.jcode.mobile.notifications.SessionNotificationPolicy
import dev.jcode.mobile.notifications.SessionNotificationStore
import dev.jcode.mobile.notifications.notificationHostKey
import java.text.DateFormat
import java.util.Date

/** Shared UI owner places this only on a connected, non-demo session. No automatic service starts. */
@Composable
fun SessionNotificationControls(host: String, sessionId: String) {
    val context = LocalContext.current
    val hostKey = remember(host) { runCatching { notificationHostKey(host) }.getOrNull() }
    if (hostKey == null) { Text("Connect to a valid gateway to configure notifications."); return }
    val store = remember(context) { SessionNotificationStore(context) }
    var policy by remember(hostKey, sessionId) { mutableStateOf(store.get(hostKey, sessionId)) }
    var message by remember(hostKey, sessionId) { mutableStateOf<String?>(null) }
    val monitor by SessionMonitorService.state.collectAsStateWithLifecycle()
    DisposableEffect(store, hostKey, sessionId) {
        val removeListener = store.listen { policy = store.get(hostKey, sessionId) }
        onDispose { removeListener() }
    }
    fun save(updated: SessionNotificationPolicy) {
        message = runCatching {
            store.set(hostKey, sessionId, updated)
            policy = updated
            if (!updated.enabled && store.enabledSessionIds(hostKey).isEmpty() && monitor.hostKey == hostKey) SessionMonitorService.stop(context)
        }.exceptionOrNull()?.message
    }
    Column(verticalArrangement = Arrangement.spacedBy(8.dp)) {
        Text("Session notifications", style = MaterialTheme.typography.titleMedium)
        Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.SpaceBetween) {
            Text("Notify for this session", modifier = Modifier.weight(1f))
            Switch(checked = policy.enabled, onCheckedChange = { save(policy.copy(enabled = it, snoozedUntil = 0)) })
        }
        Text("Opt in to completion, failure and explicit needs-input alerts. Generic idle or blocked does not mean input is needed.", style = MaterialTheme.typography.bodySmall)
        Text("Start monitoring to poll this gateway every 15 seconds in the background. A visible ongoing notice lets you stop. Uses network and battery, stops within 5h 45m or when disconnected. Android may stop it earlier. No automatic restart.", style = MaterialTheme.typography.bodySmall)
        Text("Session names and message content stay out of notifications. Needs-input alerts require explicit gateway evidence.", style = MaterialTheme.typography.bodySmall)
        if (policy.enabled) {
            if (policy.snoozedUntil > System.currentTimeMillis()) {
                Text("Snoozed until ${DateFormat.getTimeInstance(DateFormat.SHORT).format(Date(policy.snoozedUntil))}")
                TextButton(onClick = { save(policy.copy(snoozedUntil = 0)) }) { Text("Unsnooze") }
            } else {
                TextButton(onClick = { save(policy.copy(snoozedUntil = System.currentTimeMillis() + 60 * 60 * 1000L)) }) { Text("Snooze this session for 1 hour") }
            }
        }
        if (monitor.running && monitor.hostKey == hostKey) {
            Text(monitor.message)
            OutlinedButton(onClick = { SessionMonitorService.stop(context) }) { Text("Stop all monitoring") }
        } else {
            Text(if (monitor.hostKey == null || monitor.hostKey == hostKey) monitor.message else "Another gateway is being monitored")
            Button(enabled = policy.enabled, onClick = {
                message = context.mainActivity()?.startSessionMonitoring(host) ?: if (context.mainActivity() == null) "Open Jcode to start monitoring" else null
            }) { Text("Start monitoring") }
        }
        TextButton(onClick = { context.startActivity(Intent(Settings.ACTION_APP_NOTIFICATION_SETTINGS)
            .putExtra(Settings.EXTRA_APP_PACKAGE, context.packageName)) }) { Text("Android notification settings") }
        message?.let { Text(it, style = MaterialTheme.typography.bodySmall) }
    }
}

private fun Context.mainActivity(): MainActivity? = when (this) {
    is MainActivity -> this
    is ContextWrapper -> baseContext.takeIf { it !== this }?.mainActivity()
    else -> null
}
