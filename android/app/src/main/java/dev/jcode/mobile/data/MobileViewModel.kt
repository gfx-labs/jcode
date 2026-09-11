package dev.jcode.mobile.data

import android.app.Application
import dev.jcode.mobile.notifications.SessionMonitorService
import dev.jcode.mobile.notifications.notificationHostKey
import androidx.lifecycle.AndroidViewModel
import androidx.lifecycle.viewModelScope
import kotlinx.coroutines.*
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.flow.update
import kotlinx.coroutines.sync.Mutex
import kotlinx.coroutines.sync.withLock
import java.util.UUID
import java.util.concurrent.atomic.AtomicLong

class MobileViewModel(application: Application) : AndroidViewModel(application) {
    private val mutableState = MutableStateFlow(MobileState())
    val state: StateFlow<MobileState> = mutableState.asStateFlow()
    private val mutableUsage = MutableStateFlow(ProviderUsageState())
    val usage: StateFlow<ProviderUsageState> = mutableUsage.asStateFlow()
    private var usageJob: Job? = null
    private var usageRevision = 0L

    private fun stopUsage(clear: Boolean) {
        usageRevision++
        usageJob?.cancel(); usageJob = null
        mutableUsage.value = if (clear) ProviderUsageState() else mutableUsage.value.copy(loading = false)
    }

    fun refreshUsage() {
        if (usageJob?.isActive == true) return
        if (state.value.isDemo) { mutableUsage.value = ProviderUsageState(error = "Demo mode does not fetch real provider allowances."); return }
        val saved = credential
        if (!foreground || saved == null || state.value.connection != ConnectionStatus.CONNECTED) {
            mutableUsage.update { it.copy(loading = false, error = "Reconnect to load provider allowances.") }
            return
        }
        val epoch = generation
        val revision = ++usageRevision
        mutableUsage.update { it.copy(loading = true, error = null) }
        usageJob = viewModelScope.launch {
            try {
                val id = ids.getAndIncrement()
                val event = client.exchange(saved, WireCodec.request("mobile_usage", id), id)
                val result = decodeProviderUsage(event)
                if (epoch == generation && revision == usageRevision) mutableUsage.value = result
            } catch (e: CancellationException) { throw e }
            catch (e: Exception) {
                if (epoch == generation && revision == usageRevision) mutableUsage.update { it.copy(error = when (e) {
                    is GatewayUpgradeException, is IllegalArgumentException -> "Update the Jcode daemon to view provider usage."
                    is GatewayAuthenticationException -> "Pairing expired. Reconnect to your computer."
                    else -> "Could not load provider allowances. Try refreshing."
                }) }
            } finally {
                if (epoch == generation && revision == usageRevision) mutableUsage.update { it.copy(loading = false) }
            }
        }
    }

    private val appearanceStore = AppearanceStore(application)
    private val mutableAppearance = MutableStateFlow(appearanceStore.load())
    val appearance: StateFlow<AppearancePreferences> = mutableAppearance.asStateFlow()
    fun setTheme(theme: ThemeMode) { saveAppearance(mutableAppearance.value.copy(theme = theme)) }
    private fun saveAppearance(value: AppearancePreferences) {
        mutableAppearance.value = value
        appearanceStore.save(value)
    }
    private fun customizeSession(id: String, transform: (SessionCustomization) -> SessionCustomization) {
        val host = if (state.value.isDemo) "demo" else state.value.host
        val current = mutableAppearance.value
        saveAppearance(current.update(host, id, transform(current.session(host, id))))
    }
    fun togglePin(id: String) = customizeSession(id) { it.copy(pinned = !it.pinned) }
    fun renameSession(id: String, name: String) = customizeSession(id) { it.copy(name = name.trim().take(80)) }
    private val store = CredentialStore(application)
    private val client = GatewayClient()
    private val ids = AtomicLong(1)
    private var credential: Credential? = null
    private var pendingNotification: Pair<String, String>? = null
    private var navigationRequest = 0L
    private var foreground = false
    private var pollJob: Job? = null
    private var pairJob: Job? = null
    private var refreshJob: Job? = null
    private var historySession: String? = null
    private var historyAt = 0L
    private val messageJobs = mutableSetOf<Job>()
    private val refreshMutex = Mutex()
    @Volatile private var generation = 0
    private val credentialMutex = Mutex()

    // Serialize complete disk transactions, even across cancelled pairing/disconnect jobs.
    private suspend fun <T> credentialIO(block: () -> T): T = withContext(NonCancellable) {
        credentialMutex.withLock { withContext(Dispatchers.IO) { block() } }
    }

    init {
        val epoch = generation
        viewModelScope.launch {
            try {
                val saved = credentialIO { store.load() }
                if (epoch == generation) {
                    credential = saved
                    saved?.let { mutableState.value = MobileState(host = it.host, serverName = it.serverName) }
                    startPolling()
                }
            } catch (e: CancellationException) { throw e }
            catch (_: Exception) {
                if (epoch == generation) mutableState.update { it.copy(error = "Saved pairing could not be decrypted. Pair again on this device.") }
            }
        }
    }

    fun pair(host: String, code: String) {
        stopWork()
        val attempt = generation
        credential = null
        mutableState.value = MobileState(connection = ConnectionStatus.PAIRING, host = host)
        pairJob = viewModelScope.launch {
            try {
                val deviceId = withContext(Dispatchers.IO) {
                    val prefs = getApplication<Application>().getSharedPreferences("gateway_device", 0)
                    prefs.getString("id", null) ?: UUID.randomUUID().toString().also { prefs.edit().putString("id", it).apply() }
                }
                val paired = client.pair(host, code.trim(), deviceId)
                if (attempt != generation) return@launch
                credentialIO { if (attempt == generation) store.save(paired) }
                if (attempt != generation) return@launch
                credential = paired
                mutableState.value = MobileState(connection = ConnectionStatus.CONNECTING, host = paired.host, serverName = paired.serverName)
                startPolling()
            } catch (e: CancellationException) { throw e }
            catch (e: Exception) {
                if (attempt == generation) mutableState.update { it.copy(connection = ConnectionStatus.DISCONNECTED, error = e.message ?: "Pairing failed") }
            }
        }
    }

    fun setForeground(value: Boolean) {
        foreground = value
        if (value) startPolling() else {
            stopUsage(clear = false)
            pollJob?.cancel(); pollJob = null
            refreshJob?.cancel(); refreshJob = null
            messageJobs.forEach { it.cancel() }; messageJobs.clear()
            mutableState.update { MobileReducer.disconnected(it).copy(connection = if (it.isDemo) ConnectionStatus.DEMO else ConnectionStatus.DISCONNECTED) }
        }
    }

    private fun startPolling() {
        if (!foreground || credential == null || state.value.isDemo || pollJob?.isActive == true) return
        val epoch = generation
        pollJob = viewModelScope.launch {
            var failures = 0
            while (isActive && epoch == generation && credential != null) {
                val success = refreshNow()
                failures = if (success) 0 else (failures + 1).coerceAtMost(4)
                delay(if (failures == 0) 3_000L else (1_000L shl failures))
            }
        }
    }

    fun refresh() { if (foreground && !state.value.isDemo && refreshJob?.isActive != true) refreshJob = viewModelScope.launch { refreshNow() } }
    private suspend fun refreshNow(): Boolean = refreshMutex.withLock {
        val saved = credential ?: return@withLock false
        val epoch = generation
        try {
            val id = ids.getAndIncrement()
            val event = client.exchange(saved, WireCodec.request("list_sessions", id), id)
            if (generation != epoch) return@withLock false
            if (event.text("type") == "error") error(event.text("message").ifBlank { "Gateway requires a newer Jcode server" })
            check(event.text("type") == "sessions_list") { "Gateway does not support session monitoring. Update the Jcode server." }
            mutableState.update { MobileReducer.reduce(it, event).copy(connection = ConnectionStatus.CONNECTED, error = null, lastUpdatedMillis = System.currentTimeMillis()) }
            applyPendingNotificationNavigation()
            val selected = state.value.selectedSessionId
            val historyInterval = if (state.value.deliveries.any { it.sessionId == selected && it.matchedHistoryIndex == null && it.status != DeliveryStatus.REJECTED }) 3_000L else 15_000L
            if (selected != null && state.value.sessions.any { it.id == selected } && (historySession != selected || System.currentTimeMillis() - historyAt >= historyInterval)) {
                historySession = selected
                historyAt = System.currentTimeMillis()
                val historyId = ids.getAndIncrement()
                val history = client.exchange(saved, WireCodec.request("comm_read_context", historyId, "session_id" to selected, "target_session" to selected), historyId)
                if (generation == epoch) {
                    // A busy agent cannot provide persisted history. Preserve cached transcript and use live output_tail.
                    if (history.text("type") != "error" || !history.text("message").contains("busy", ignoreCase = true))
                        mutableState.update { MobileReducer.reduce(it, history, selected) }
                }
            }
            true
        } catch (e: GatewayAuthenticationException) {
            if (epoch == generation) {
                SessionMonitorService.stop(getApplication())
                credential = null
                runCatching { credentialIO { if (epoch == generation) store.clear() } }
                if (epoch == generation) mutableState.update { it.copy(connection = ConnectionStatus.DISCONNECTED, error = e.message) }
            }
            false
        } catch (e: GatewayUpgradeException) {
            if (epoch == generation) {
                SessionMonitorService.stop(getApplication())
                credential = null
                mutableState.update { it.copy(connection = ConnectionStatus.DISCONNECTED, error = e.message) }
            }
            false
        } catch (e: TimeoutCancellationException) {
            if (epoch == generation) mutableState.update { it.copy(connection = ConnectionStatus.RECONNECTING, error = "Gateway timed out. Reconnecting…") }
            false
        } catch (e: CancellationException) { throw e }
        catch (e: Exception) {
            if (epoch == generation) mutableState.update { it.copy(connection = ConnectionStatus.RECONNECTING, error = e.message ?: "Gateway unavailable") }
            false
        }
    }

    /** A notification can navigate only within the currently paired workspace, never send. */
    fun openNotificationSession(hostKey: String, sessionId: String) {
        if (!hostKey.matches(Regex("[a-f0-9]{64}")) || sessionId.isBlank() || sessionId.length > 512 || state.value.isDemo) return
        pendingNotification = hostKey to sessionId
        if (state.value.connection == ConnectionStatus.CONNECTED) applyPendingNotificationNavigation()
        refresh()
    }
    private fun applyPendingNotificationNavigation() {
        val pending = pendingNotification ?: return
        val saved = credential ?: return // Cold launch: wait for saved pairing and the first session list.
        pendingNotification = null
        if (state.value.isDemo || runCatching { notificationHostKey(saved.host) }.getOrNull() != pending.first) return
        if (state.value.sessions.none { it.id == pending.second }) {
            mutableState.update { it.copy(error = "That session is no longer available.") }
            return
        }
        historySession = null; historyAt = 0L
        navigationRequest++
        mutableState.update { it.copy(selectedSessionId = pending.second, openThreadRequest = navigationRequest) }
    }

    fun selectSession(id: String) {
        mutableState.update { it.copy(selectedSessionId = id) }
        refresh()
    }
    fun sendMessage(sessionId: String, text: String) = broadcast(listOf(sessionId), text)

    /** Explicit, one attempt per recipient. Timeouts mean UNKNOWN, never retry a message. */
    fun broadcast(sessionIds: List<String>, text: String) {
        if (text.isBlank() || sessionIds.isEmpty()) return
        val snapshot = state.value
        val saved = credential
        val epoch = generation
        val sentAtUnixMs = System.currentTimeMillis()
        sessionIds.distinct().forEach { target ->
            val id = ids.getAndIncrement()
            val allowed = saved != null && foreground && snapshot.connection == ConnectionStatus.CONNECTED && !snapshot.isDemo
            val result = DeliveryResult(id, target, snapshot.sessions.firstOrNull { it.id == target }?.name ?: target, text,
                if (allowed) DeliveryStatus.SENDING else DeliveryStatus.REJECTED,
                if (snapshot.isDemo) "Demo only. No message was sent." else if (!allowed) "Not connected. No message was sent." else "Awaiting server confirmation",
                historyBaseline = snapshot.transcripts[target]?.let { historyBaseline(it) },
                insertionIndex = snapshot.transcripts[target]?.count { !it.isStreaming },
                outputAtSend = snapshot.sessions.firstOrNull { it.id == target }?.output,
                sentAtUnixMs = sentAtUnixMs)
            mutableState.update { it.copy(deliveries = (it.deliveries + result).takeLast(1000)) }
            if (allowed) {
                val job = viewModelScope.launch {
                    try {
                        val event = client.exchange(saved!!, WireCodec.request("mobile_message", id, "session_id" to target, "content" to text), id)
                        if (epoch == generation) {
                            if (event.text("type") == "mobile_delivery" || event.text("type") == "error") mutableState.update { MobileReducer.reduce(it, event) }
                            else markUnknown(id, "Unexpected server reply. Not resent.")
                        }
                    } catch (e: CancellationException) {
                        if (epoch == generation) markUnknown(id, "Confirmation interrupted. Not resent.")
                        throw e
                    } catch (_: Exception) {
                        if (epoch == generation) markUnknown(id, "No confirmation. Delivery may have occurred. Not resent.")
                    }
                }
                messageJobs += job
                job.invokeOnCompletion { viewModelScope.launch { messageJobs.remove(job) } }
            }
        }
    }
    private fun markUnknown(id: Long, detail: String) {
        mutableState.update { it.copy(deliveries = it.deliveries.map { result -> if (result.requestId == id && result.status == DeliveryStatus.SENDING) result.copy(status = DeliveryStatus.UNKNOWN, detail = detail) else result }) }
    }
    private fun stopWork() {
        pendingNotification = null
        SessionMonitorService.stop(getApplication())
        generation++
        stopUsage(clear = true)
        pollJob?.cancel(); pollJob = null
        pairJob?.cancel(); pairJob = null
        refreshJob?.cancel(); refreshJob = null
        historySession = null; historyAt = 0L
        messageJobs.forEach { it.cancel() }; messageJobs.clear()
        client.close()
    }
    fun disconnect() {
        val leavingDemo = state.value.isDemo
        stopWork()
        if (leavingDemo) {
            val saved = credential
            mutableState.value = MobileState(host = saved?.host.orEmpty(), serverName = saved?.serverName ?: "Jcode")
            startPolling()
            return
        }
        credential = null
        val epoch = generation
        mutableState.value = MobileState()
        viewModelScope.launch(start = CoroutineStart.UNDISPATCHED) {
            try { credentialIO { store.clear() } }
            catch (_: Exception) {
                if (epoch == generation) mutableState.value = MobileState(error = "Could not erase saved pairing. Clear app storage to remove it.")
            }
        }
    }
    fun dismissError() { mutableState.update { it.copy(error = null) } }
    fun loadDemo() {
        stopWork()
        mutableState.value = DemoState.create()
    }
    override fun onCleared() { stopWork(); super.onCleared() }
}

/** Explicit screenshot/demo data. Never selected automatically on network failure. */
private object DemoState {
    fun create(): MobileState {
        val sessions = listOf(
            MobileSession("demo-root", "Atlas", "running", "Shipping the Android companion", "Android companion", "coordinator", model = "Claude Opus 4.6", isProcessing = true,
                todos = listOf(MobileTodo("Design session control center", "completed"), MobileTodo("Connect live gateway", "in_progress"), MobileTodo("Build and verify APK", "pending")), todosCompleted = 1, todosTotal = 3, tokens = 84200,
                workingDirectory = "/home/demo/projects/jcode", lastActivityAgeSecs = 2,
                output = "Gateway integration is ready. Verifying delivery confirmations and reconnection behavior."),
            MobileSession("demo-api", "Nova", "running", "Testing secure websocket transport", "Gateway & networking", parentId = "demo-root", model = "GPT-5", isProcessing = true, currentTool = "bash", todosCompleted = 2, todosTotal = 3,
                todos = listOf(MobileTodo("Pair with host", "completed"), MobileTodo("Persist credentials securely", "completed"), MobileTodo("Exercise reconnect cases", "in_progress")), workingDirectory = "/home/demo/projects/jcode", lastActivityAgeSecs = 0, output = "12 protocol tests passed. Checking the final reconnect case."),
            MobileSession("demo-ui", "Pixel", "completed", "Compose screens are ready", "Mobile interface", parentId = "demo-root", model = "Claude Sonnet 4.6", todosCompleted = 3, todosTotal = 3,
                workingDirectory = "/home/demo/projects/jcode/android", lastActivityAgeSecs = 300,
                completionReport = "Built the session dashboard, transcript and recipient picker. Verified all screen sizes."),
            MobileSession("demo-other", "Orion", "ready", "Waiting for your next instruction", "API cleanup", model = "GPT-5", todosCompleted = 4, todosTotal = 4, workingDirectory = "/home/demo/projects/api", lastActivityAgeSecs = 3600)
        )
        return MobileState(ConnectionStatus.DEMO, "demo.local:7643", "Demo workspace", sessions, "demo-root", mapOf(
            "demo-root" to listOf(TranscriptEntry("demo-1", "user", "Build a native Android companion for my running sessions."), TranscriptEntry("demo-2", "assistant", "I’ve split the work into interface and gateway tasks. You can inspect each agent and send a targeted update below.", tools = listOf(ToolEntry("demo-tool", "swarm", "Spawn gateway and UI workers", "Nova and Pixel are running", "succeeded")))),
            "demo-api" to listOf(TranscriptEntry("demo-3", "assistant", "Pairing credentials are encrypted with Android Keystore. Messages are never resent automatically."))
        ), isDemo = true)
    }
}
