package dev.jcode.mobile.notifications

import android.Manifest
import android.app.Notification
import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.PendingIntent
import android.app.Service
import android.content.BroadcastReceiver
import android.content.Context
import android.content.Intent
import android.content.pm.PackageManager
import android.content.pm.ServiceInfo
import android.net.Uri
import android.os.Build
import android.os.IBinder
import androidx.activity.ComponentActivity
import androidx.lifecycle.Lifecycle
import dev.jcode.mobile.MainActivity
import dev.jcode.mobile.R
import dev.jcode.mobile.data.CredentialStore
import dev.jcode.mobile.data.GatewayAuthenticationException
import dev.jcode.mobile.data.GatewayClient
import dev.jcode.mobile.data.GatewayUpgradeException
import dev.jcode.mobile.data.WireCodec
import dev.jcode.mobile.data.objects
import dev.jcode.mobile.data.text
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.TimeoutCancellationException
import kotlinx.coroutines.cancel
import kotlinx.coroutines.delay
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.isActive
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import java.util.concurrent.atomic.AtomicLong

data class SessionMonitorState(val running: Boolean = false, val hostKey: String? = null, val message: String = "Monitoring stopped")

/** User-initiated snapshot fetching, not a connected-device service or agent attachment.
 * No boot receiver, WorkManager, alarms, sticky restart, message sends or credential-bearing intents.
 * Android 15 dataSync budget: https://developer.android.com/develop/background-work/services/fgs/timeout
 */
class SessionMonitorService : Service() {
    private val scope = CoroutineScope(SupervisorJob() + Dispatchers.Main.immediate)
    private val client = GatewayClient()
    private var started = false
    private var epoch = -1L
    private var hostKey = ""

    override fun onCreate() { super.onCreate(); serviceAlive = true }

    override fun onBind(intent: Intent?): IBinder? = null

    override fun onStartCommand(intent: Intent?, flags: Int, startId: Int): Int {
        val requestedEpoch = intent?.getLongExtra(EXTRA_EPOCH, -1) ?: -1
        if (intent?.action != ACTION_START || requestedEpoch != generation.get() || requestedEpoch < 0) {
            if (!started) stopSelf()
            return START_NOT_STICKY
        }
        if (started) return START_NOT_STICKY
        epoch = requestedEpoch
        hostKey = intent.getStringExtra(EXTRA_HOST_KEY).orEmpty()
        try {
            createChannels(this)
            if (!notificationsAllowed(this)) { finish("Notifications are disabled"); return START_NOT_STICKY }
            val ongoing = Notification.Builder(this, CHANNEL_MONITOR)
                .setSmallIcon(R.drawable.ic_launcher).setContentTitle("Jcode session monitoring")
                .setContentText("Read-only polling every 15 seconds. Stops within 5h 45m.")
                .setContentIntent(openIntent(this)).setOngoing(true).setOnlyAlertOnce(true)
                .setVisibility(Notification.VISIBILITY_PUBLIC)
                .addAction(Notification.Action.Builder(null, "Stop monitoring", actionIntent(this, ACTION_STOP)).build())
                .build()
            if (Build.VERSION.SDK_INT >= 29) startForeground(ONGOING_ID, ongoing, ServiceInfo.FOREGROUND_SERVICE_TYPE_DATA_SYNC)
            else startForeground(ONGOING_ID, ongoing)
        } catch (_: Exception) { finish("Android could not start monitoring. Open the app and try again."); return START_NOT_STICKY }
        started = true
        mutableState.value = SessionMonitorState(true, hostKey, "Monitoring every 15 seconds")
        scope.launch { delay(MAX_RUN_MILLIS); finish("Monitoring time limit reached. Open the app to start again.") }
        scope.launch { monitor() }
        return START_NOT_STICKY
    }

    private suspend fun monitor() {
        val store = SessionNotificationStore(this)
        val credentials = CredentialStore(this)
        val detector = SessionTransitions()
        var failures = 0
        var requestId = 1L
        try {
            val initial = withContext(Dispatchers.IO) { credentials.load() }
            if (initial == null || notificationHostKey(initial.host) != hostKey) { finish("Pairing changed. Monitoring stopped."); return }
            while (scope.isActive && epoch == generation.get()) {
                if (!notificationsAllowed(this)) { finish("Notifications are disabled"); return }
                val watched = store.enabledSessionIds(hostKey)
                if (watched.isEmpty()) { finish("No sessions have notifications enabled"); return }
                if (withContext(Dispatchers.IO) { credentials.load() } != initial) { finish("Pairing changed. Monitoring stopped."); return }
                if (epoch != generation.get() || !scope.isActive) return
                try {
                    val id = requestId++
                    val response = client.exchange(initial, WireCodec.request("list_sessions", id), id)
                    if (epoch != generation.get() || !scope.isActive) return
                    if (withContext(Dispatchers.IO) { credentials.load() } != initial) { finish("Pairing changed. Monitoring stopped."); return }
                    if (epoch != generation.get() || !scope.isActive) return
                    if (response.text("type") != "sessions_list") { finish("Gateway cannot monitor sessions. Update or pair again."); return }
                    // Only watched IDs retained by the detector. Baselines still advance while snoozed.
                    val sessions = response.objects("sessions").filter { it.text("session_id") in watched }.map(WireCodec::session)
                    detector.observe(sessions, watched).forEach { (sessionId, alert) ->
                        if (epoch == generation.get() && store.get(hostKey, sessionId).allows(System.currentTimeMillis())) {
                            showAlert(sessionId, alert)
                        }
                    }
                    failures = 0
                    mutableState.value = SessionMonitorState(true, hostKey, "Monitoring every 15 seconds")
                } catch (_: GatewayAuthenticationException) { finish("Pairing expired. Monitoring stopped."); return }
                catch (_: GatewayUpgradeException) { finish("Gateway update required. Monitoring stopped."); return }
                catch (_: TimeoutCancellationException) { failures++ }
                catch (e: CancellationException) { throw e }
                catch (_: Exception) { failures++ }
                if (epoch != generation.get() || !scope.isActive) return
                if (failures >= 3) { finish("Gateway disconnected. Open the app to start monitoring again."); return }
                if (failures > 0) mutableState.value = SessionMonitorState(true, hostKey, "Gateway unavailable. Retrying ($failures/3)")
                delay(if (failures == 0) 15_000L else 15_000L shl failures)
            }
        } catch (e: CancellationException) { throw e }
        catch (_: Exception) { finish("Monitoring stopped. Check pairing and notification settings.") }
    }

    private fun showAlert(sessionId: String, alert: SessionAlert) {
        // Even unlocked text omits host, session names, output, paths, prompts, and tokens.
        val notification = Notification.Builder(this, CHANNEL_ALERTS)
            .setSmallIcon(R.drawable.ic_launcher).setContentTitle(alert.title)
            .setContentText("Open Jcode to view the session.").setVisibility(Notification.VISIBILITY_PRIVATE)
            .setPublicVersion(Notification.Builder(this, CHANNEL_ALERTS).setSmallIcon(R.drawable.ic_launcher)
                .setContentTitle("Jcode session update").setContentText("Open Jcode to view.").build())
            .setContentIntent(openIntent(this, hostKey, sessionId)).setAutoCancel(true)
            .addAction(Notification.Action.Builder(null, "Open session", openIntent(this, hostKey, sessionId)).build())
            .addAction(Notification.Action.Builder(null, "Snooze 1 hour", actionIntent(this, ACTION_SNOOZE, hostKey, sessionId)).build())
            .addAction(Notification.Action.Builder(null, "Mute session", actionIntent(this, ACTION_MUTE, hostKey, sessionId)).build())
            .build()
        getSystemService(NotificationManager::class.java).notify(alertTag(hostKey, sessionId), ALERT_ID, notification)
    }

    private fun finish(message: String) {
        if (epoch == generation.get()) mutableState.value = SessionMonitorState(false, hostKey.takeIf { it.isNotEmpty() }, message)
        scope.cancel()
        client.close()
        stopForeground(STOP_FOREGROUND_REMOVE)
        stopSelf()
    }

    /** API 35: stop immediately, not after network/coroutine cleanup or a retry. */
    override fun onTimeout(startId: Int, fgsType: Int) { finish("Android monitoring time limit reached. Open the app to start again.") }
    override fun onTaskRemoved(rootIntent: Intent?) { finish("Monitoring stopped when the app was dismissed") }
    override fun onDestroy() {
        serviceAlive = false
        scope.cancel()
        client.close()
        if (epoch == generation.get() && mutableState.value.running) mutableState.value = SessionMonitorState(false, hostKey, "Monitoring stopped")
        super.onDestroy()
    }

    companion object {
        const val ACTION_OPEN = "dev.jcode.mobile.OPEN_MONITORED_SESSION"
        internal const val ACTION_STOP = "dev.jcode.mobile.STOP_MONITOR"
        internal const val ACTION_SNOOZE = "dev.jcode.mobile.SNOOZE_SESSION"
        internal const val ACTION_MUTE = "dev.jcode.mobile.MUTE_SESSION"
        private const val ACTION_START = "dev.jcode.mobile.START_MONITOR"
        private const val EXTRA_EPOCH = "monitor_generation"
        private const val EXTRA_HOST_KEY = "host_key"
        private const val CHANNEL_MONITOR = "session_monitor_v1"
        private const val CHANNEL_ALERTS = "session_alerts_v1"
        private const val ONGOING_ID = 7600
        internal const val ALERT_ID = 7601
        private const val MAX_RUN_MILLIS = 5 * 60 * 60 * 1000L + 45 * 60 * 1000L
        private val generation = AtomicLong(0)
        private var serviceAlive = false
        private val mutableState = MutableStateFlow(SessionMonitorState())
        val state = mutableState.asStateFlow()

        /** Only call directly from a user's tap in a resumed activity. Never on lifecycle resume. */
        fun start(activity: ComponentActivity, host: String): String? {
            if (!activity.lifecycle.currentState.isAtLeast(Lifecycle.State.RESUMED)) return "Open the app before starting monitoring"
            createChannels(activity)
            if (!notificationsAllowed(activity)) return "Allow notifications in Android settings first"
            val key = runCatching { notificationHostKey(host) }.getOrNull() ?: return "Invalid gateway host"
            if (SessionNotificationStore(activity).enabledSessionIds(key).isEmpty()) return "Enable notifications for a session first"
            if (state.value.running && state.value.hostKey == key) return null
            if (state.value.running) return "Stop the other gateway's monitoring first"
            if (serviceAlive) return "Monitoring is stopping. Tap Start monitoring again shortly."
            val epoch = generation.incrementAndGet()
            mutableState.value = SessionMonitorState(true, key, "Starting monitoring…")
            return try {
                activity.startForegroundService(Intent(activity, SessionMonitorService::class.java).setAction(ACTION_START)
                    .putExtra(EXTRA_HOST_KEY, key).putExtra(EXTRA_EPOCH, epoch))
                null
            } catch (_: Exception) {
                mutableState.value = SessionMonitorState(message = "Android blocked monitoring. Keep the app open and try again.")
                mutableState.value.message
            }
        }

        fun stop(context: Context) {
            generation.incrementAndGet() // In-flight replies and queued starts cannot alert after disconnect.
            mutableState.value = SessionMonitorState(message = "Monitoring stopped")
            context.stopService(Intent(context, SessionMonitorService::class.java))
        }

        fun notificationsAllowed(context: Context): Boolean {
            if (Build.VERSION.SDK_INT >= 33 && context.checkSelfPermission(Manifest.permission.POST_NOTIFICATIONS) != PackageManager.PERMISSION_GRANTED) return false
            val manager = context.getSystemService(NotificationManager::class.java)
            return manager.areNotificationsEnabled() && listOf(CHANNEL_MONITOR, CHANNEL_ALERTS).all {
                manager.getNotificationChannel(it)?.importance != NotificationManager.IMPORTANCE_NONE
            }
        }

        fun createChannels(context: Context) {
            context.getSystemService(NotificationManager::class.java).createNotificationChannels(listOf(
                NotificationChannel(CHANNEL_MONITOR, "Background session monitoring", NotificationManager.IMPORTANCE_LOW),
                NotificationChannel(CHANNEL_ALERTS, "Session completion, failure and input", NotificationManager.IMPORTANCE_DEFAULT)
            ))
        }

        internal fun alertTag(hostKey: String, sessionId: String) = "session:$hostKey:$sessionId"
        private fun targetUri(hostKey: String, sessionId: String): Uri = Uri.Builder().scheme("jcode-notification")
            .authority("session").appendPath(hostKey).appendPath(sessionId).build()
        private fun openIntent(context: Context, hostKey: String = "", sessionId: String = ""): PendingIntent = PendingIntent.getActivity(context, 0,
            Intent(context, MainActivity::class.java).setAction(ACTION_OPEN).setData(targetUri(hostKey, sessionId))
                .addFlags(Intent.FLAG_ACTIVITY_CLEAR_TOP or Intent.FLAG_ACTIVITY_SINGLE_TOP), PendingIntent.FLAG_UPDATE_CURRENT or PendingIntent.FLAG_IMMUTABLE)
        private fun actionIntent(context: Context, action: String, hostKey: String = "", sessionId: String = ""): PendingIntent = PendingIntent.getBroadcast(context, 0,
            Intent(context, SessionNotificationActionReceiver::class.java).setAction(action).setData(targetUri(hostKey, sessionId)),
            PendingIntent.FLAG_UPDATE_CURRENT or PendingIntent.FLAG_IMMUTABLE)
    }
}

/** Notification actions update local policy only. They never start or restart background work. */
class SessionNotificationActionReceiver : BroadcastReceiver() {
    override fun onReceive(context: Context, intent: Intent) {
        if (intent.action == SessionMonitorService.ACTION_STOP) { SessionMonitorService.stop(context); return }
        if (intent.action !in setOf(SessionMonitorService.ACTION_MUTE, SessionMonitorService.ACTION_SNOOZE)) return
        val parts = intent.data?.pathSegments ?: return
        if (parts.size != 2 || !parts[0].matches(Regex("[a-f0-9]{64}")) || parts[1].isBlank()) return
        val (hostKey, sessionId) = parts
        val store = SessionNotificationStore(context)
        val old = store.get(hostKey, sessionId)
        val updated = if (intent.action == SessionMonitorService.ACTION_MUTE) old.copy(enabled = false)
            else old.copy(snoozedUntil = System.currentTimeMillis() + 60 * 60 * 1000L)
        if (runCatching { store.set(hostKey, sessionId, updated) }.isFailure) return
        context.getSystemService(NotificationManager::class.java).cancel(SessionMonitorService.alertTag(hostKey, sessionId), SessionMonitorService.ALERT_ID)
        if (store.enabledSessionIds(hostKey).isEmpty() && SessionMonitorService.state.value.hostKey == hostKey) SessionMonitorService.stop(context)
    }
}
