@file:OptIn(androidx.compose.material3.ExperimentalMaterial3Api::class)

package dev.jcode.mobile.ui

import androidx.activity.compose.BackHandler
import androidx.compose.foundation.horizontalScroll
import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.LazyListState
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.lazy.rememberLazyListState
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.foundation.text.selection.SelectionContainer
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.outlined.ArrowBack
import androidx.compose.material.icons.automirrored.outlined.Send
import androidx.compose.material.icons.outlined.*
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.runtime.saveable.mapSaver
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.layout.onGloballyPositioned
import androidx.compose.ui.layout.positionInWindow
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.semantics.selected
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.input.KeyboardType
import androidx.compose.ui.text.input.PasswordVisualTransformation
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.window.layout.FoldingFeature
import dev.jcode.mobile.data.*
import androidx.lifecycle.Lifecycle
import androidx.lifecycle.compose.LocalLifecycleOwner
import androidx.lifecycle.repeatOnLifecycle
import kotlinx.coroutines.delay
import kotlinx.coroutines.flow.collectLatest
import kotlinx.coroutines.flow.distinctUntilChanged
import kotlinx.coroutines.flow.drop
import kotlinx.coroutines.flow.collect
import kotlinx.coroutines.launch

@Composable
fun JcodeApp(state: MobileState, vm: MobileViewModel, fold: FoldingFeature?) {
    Workspace(state, fold, WorkspaceActions(vm::selectSession, vm::refresh, vm::sendMessage,
        vm::broadcast, vm::dismissError, loadDemo = vm::loadDemo, togglePin = vm::togglePin, renameSession = vm::renameSession), usageContent = { ProviderUsagePage(vm.usage.collectAsState().value, state, vm::refreshUsage) }) { SettingsPanel(state, vm) }
}

@Composable
internal fun Workspace(state: MobileState, fold: FoldingFeature?, actions: WorkspaceActions,
    usageContent: @Composable () -> Unit = {}, settingsContent: @Composable () -> Unit = {}) {
    var usageOpen by rememberSaveable { mutableStateOf(false) }
    var settingsOpen by rememberSaveable { mutableStateOf(false) }
    var phoneDetail by rememberSaveable { mutableStateOf(state.selectedSessionId != null) }
    var detailTab by rememberSaveable { mutableIntStateOf(0) }
    var broadcastOpen by rememberSaveable { mutableStateOf(false) }
    var deliveryOpen by rememberSaveable { mutableStateOf(false) }
    var deliveryAfter by rememberSaveable { mutableLongStateOf(0L) }
    val drafts = rememberSaveable(saver = mapSaver<MutableMap<String, String>>(
        save = { it.toMap() }, restore = { saved -> saved.mapValues { it.value as String }.toMutableMap() }
    )) { mutableMapOf<String, String>() }
    var draftRevision by remember { mutableIntStateOf(0) }
    val selected = state.selectedSession
    val threadState = rememberSaveable(selected?.id, saver = LazyListState.Saver) { LazyListState() }
    val threadFollowLatest = rememberSaveable(selected?.id) { mutableStateOf(true) }
    var sessionQuery by rememberSaveable { mutableStateOf("") }
    var activeOnly by rememberSaveable { mutableStateOf(false) }
    val currentDraft = draftRevision.let { drafts[selected?.id].orEmpty() }
    val changeDraft: (String) -> Unit = { text -> selected?.let { drafts[it.id] = text; draftRevision++ } }
    val send: () -> Unit = { selected?.let {
        val text = drafts[it.id].orEmpty()
        if (text.isNotBlank()) { actions.sendMessage(it.id, text); drafts[it.id] = ""; draftRevision++ }
    } }
    val select: (MobileSession) -> Unit = {
        actions.selectSession(it.id); phoneDetail = true; settingsOpen = false; detailTab = 0
    }
    val connected = state.connection == ConnectionStatus.CONNECTED || state.isDemo
    val readStore = rememberThreadReadStore()
    LaunchedEffect(state.readScope(), state.sessions) { readStore.observeActivity(state.readScope(), state.sessions) }
    LaunchedEffect(state.openThreadRequest) {
        if (state.openThreadRequest > 0) { phoneDetail = true; settingsOpen = false; usageOpen = false; detailTab = 0 }
    }
    CompositionLocalProvider(LocalThreadReadStore provides readStore, LocalThreadReadingAllowed provides (!broadcastOpen && !deliveryOpen)) {
    Scaffold(containerColor = Mist) { padding ->
        Column(Modifier.fillMaxSize().padding(padding).consumeWindowInsets(padding).imePadding()) {
            if (state.error != null) Surface(color = MaterialTheme.colorScheme.errorContainer) {
                Row(Modifier.fillMaxWidth().padding(start = 12.dp), verticalAlignment = Alignment.CenterVertically) {
                    Text(state.error, Modifier.weight(1f), style = MaterialTheme.typography.bodySmall, maxLines = 2, overflow = TextOverflow.Ellipsis)
                    IconButton(onClick = actions.dismissError) { Icon(Icons.Outlined.Close, "Dismiss error") }
                }
            }
            BoxWithConstraints(Modifier.weight(1f).fillMaxWidth()) {
                var origin by remember { mutableStateOf(Offset.Zero) }
                val density = LocalDensity.current
                val usableFold = fold?.takeIf { it.isSeparating || it.occlusionType == FoldingFeature.OcclusionType.FULL || it.state == FoldingFeature.State.HALF_OPENED }
                val localFold = usableFold?.let {
                    val vertical = it.orientation == FoldingFeature.Orientation.VERTICAL
                    FoldBounds(vertical, (if (vertical) it.bounds.left - origin.x else it.bounds.top - origin.y) / density.density,
                        (if (vertical) it.bounds.right - origin.x else it.bounds.bottom - origin.y) / density.density)
                }
                val plan = adaptivePanePlan(maxWidth.value, maxHeight.value, density.fontScale, localFold)
                BackHandler(settingsOpen || usageOpen || detailTab != 0 || (phoneDetail && plan.mode != PaneMode.DUAL)) {
                    if (usageOpen) usageOpen = false else if (settingsOpen) settingsOpen = false else if (detailTab != 0) detailTab = 0 else phoneDetail = false
                }
                val listPane: @Composable () -> Unit = {
                    Column(Modifier.fillMaxSize()) {
                        AppHeader(state, actions.refresh, { settingsOpen = true; usageOpen = false }, { usageOpen = true; settingsOpen = false })
                        if (!connected && state.sessions.isEmpty()) WelcomePanel(state, { settingsOpen = true }, actions.loadDemo)
                        else SessionPanel(state, selected?.id, select, { broadcastOpen = true }, query = sessionQuery, onQuery = { sessionQuery = it }, activeOnly = activeOnly, onActiveOnly = { activeOnly = it })
                    }
                }
                val settingsPane: @Composable () -> Unit = {
                    Column(Modifier.fillMaxSize()) {
                        Row(Modifier.fillMaxWidth().heightIn(min = 56.dp), verticalAlignment = Alignment.CenterVertically) {
                            IconButton(onClick = { settingsOpen = false; usageOpen = false }) { Icon(Icons.AutoMirrored.Outlined.ArrowBack, if (usageOpen) "Back from usage" else "Back from settings") }
                            Text(if (usageOpen) "Provider usage" else "Connection settings", style = MaterialTheme.typography.titleMedium)
                        }
                        Box(Modifier.weight(1f)) { if (usageOpen) usageContent() else settingsContent() }
                    }
                }
                val detailPane: @Composable (Boolean) -> Unit = { showBack ->
                    if (settingsOpen || usageOpen) settingsPane()
                    else SessionDetail(state, selected, detailTab, { detailTab = it }, currentDraft, draftRevision,
                        changeDraft, send, select, if (showBack) ({ phoneDetail = false }) else null, threadState = threadState, threadFollowLatest = threadFollowLatest, onPin = { selected?.let { actions.togglePin(it.id) } }, onRename = { name -> selected?.let { actions.renameSession(it.id, name) } })
                }
                Box(Modifier.fillMaxSize().onGloballyPositioned { origin = it.positionInWindow() }) {
                    when (plan.mode) {
                        PaneMode.DUAL -> Row(Modifier.fillMaxSize()) {
                            Box(Modifier.width(plan.first.dp).fillMaxHeight().testTag("session-list-pane")) { listPane() }
                            Box(Modifier.width(plan.gap.dp).fillMaxHeight().background(Line).testTag("hinge-gap"))
                            Box(Modifier.weight(1f).fillMaxHeight().testTag("conversation-pane")) { detailPane(false) }
                        }
                        PaneMode.TABLETOP -> Column(Modifier.fillMaxSize()) {
                            Box(Modifier.fillMaxWidth().height(plan.first.dp).testTag("tabletop-thread")) {
                                if (settingsOpen || usageOpen) settingsPane()
                                else if (selected != null && phoneDetail) SessionDetail(state, selected, detailTab, { detailTab = it }, currentDraft, draftRevision,
                                    changeDraft, send, select, { phoneDetail = false }, showComposer = false, threadState = threadState, threadFollowLatest = threadFollowLatest, onPin = { selected?.let { actions.togglePin(it.id) } }, onRename = { name -> selected?.let { actions.renameSession(it.id, name) } })
                                else listPane()
                            }
                            Box(Modifier.fillMaxWidth().height(plan.gap.dp).background(Line).testTag("hinge-gap"))
                            Column(Modifier.fillMaxWidth().weight(1f).background(MaterialTheme.colorScheme.surface).verticalScroll(rememberScrollState()).testTag("tabletop-controls"), verticalArrangement = Arrangement.Bottom) {
                                if (selected != null && phoneDetail && !settingsOpen && !usageOpen) {
                                    Row(Modifier.fillMaxWidth().padding(horizontal = 12.dp), verticalAlignment = Alignment.CenterVertically) {
                                        IconButton(onClick = { phoneDetail = false }) { Icon(Icons.Outlined.List, "Choose a session") }
                                        Text("Reply to ${selected.name}", Modifier.weight(1f), style = MaterialTheme.typography.bodyMedium, maxLines = 1, overflow = TextOverflow.Ellipsis)
                                        TextButton(onClick = { detailTab = if (detailTab == 0) 1 else 0 }) { Text(if (detailTab == 0) "Details" else "Thread") }
                                    }
                                    SessionComposer(state, selected, currentDraft, changeDraft, send)
                                } else {
                                    EmptyPanel(if (usageOpen) "Provider usage above" else if (settingsOpen) "Connection settings above" else "Choose a session above", "Messages stay above the fold. Reply controls appear here.")
                                }
                            }
                        }
                        PaneMode.SINGLE -> {
                            val paneModifier = if (plan.singleAlongHeight) Modifier.fillMaxWidth().height(plan.first.dp).offset(y = plan.singleOffset.dp)
                                else Modifier.width(plan.first.dp).fillMaxHeight().offset(x = plan.singleOffset.dp)
                            Box(paneModifier.testTag("single-pane")) {
                                when {
                                    settingsOpen || usageOpen -> settingsPane()
                                    phoneDetail && selected != null -> detailPane(true)
                                    else -> listPane()
                                }
                            }
                        }
                    }
                }
            }
        }
    }
    }
    if (broadcastOpen) BroadcastDialog(state, onDismiss = { broadcastOpen = false }, onSend = { recipients, text ->
        deliveryAfter = state.deliveries.maxOfOrNull { it.requestId } ?: 0L
        actions.broadcast(recipients, text); broadcastOpen = false; deliveryOpen = true
    })
    if (deliveryOpen) AlertDialog(onDismissRequest = { deliveryOpen = false }, title = { Text("Delivery results") }, text = {
        LazyColumn(Modifier.heightIn(max = 440.dp), verticalArrangement = Arrangement.spacedBy(10.dp)) {
            item { Text("Each session confirms independently. Unconfirmed messages are never resent automatically.") }
            items(state.deliveries.filter { it.requestId > deliveryAfter }, key = { "${it.requestId}:${it.sessionId}" }) { DeliveryRow(it) }
        }
    }, confirmButton = { TextButton(onClick = { deliveryOpen = false }) { Text("Done") } })
}

@Composable
private fun AppHeader(state: MobileState, onRefresh: () -> Unit, onSettings: () -> Unit, onUsage: () -> Unit) {
    Row(Modifier.fillMaxWidth().background(MaterialTheme.colorScheme.surface).heightIn(min = 52.dp).padding(start = 16.dp, end = 4.dp), verticalAlignment = Alignment.CenterVertically) {
        Column(Modifier.weight(1f)) {
            Text("Sessions", style = MaterialTheme.typography.titleLarge, fontWeight = FontWeight.Bold)
            Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(5.dp)) {
                val online = state.connection == ConnectionStatus.CONNECTED
                Box(Modifier.size(6.dp).background(if (online) Teal else if (state.isDemo) Cobalt else Amber, CircleShape))
                Text(if (state.isDemo) "Demo workspace" else when (state.connection) {
                    ConnectionStatus.CONNECTED -> state.serverName
                    ConnectionStatus.RECONNECTING -> "Reconnecting…"
                    ConnectionStatus.CONNECTING, ConnectionStatus.PAIRING -> "Connecting…"
                    else -> "Offline · cached sessions"
                }, style = MaterialTheme.typography.bodySmall, color = Muted, maxLines = 1, overflow = TextOverflow.Ellipsis)
            }
        }
        IconButton(onClick = onRefresh, enabled = state.connection != ConnectionStatus.PAIRING) { Icon(Icons.Outlined.Refresh, "Refresh sessions", Modifier.size(21.dp)) }
        IconButton(onClick = onUsage) { Icon(Icons.Outlined.PieChart, "Provider usage", tint = Ink) }
        IconButton(onClick = onSettings) { Icon(Icons.Outlined.Settings, "Connection settings", Modifier.size(21.dp)) }
    }
}

@Composable
private fun StatusPill(label: String, color: Color = Teal) {
    Row(Modifier.background(color.copy(alpha = .09f), RoundedCornerShape(50)).padding(horizontal = 10.dp, vertical = 6.dp), verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(6.dp)) {
        Box(Modifier.size(6.dp).background(color, CircleShape))
        Text(label, style = MaterialTheme.typography.labelSmall, color = color, maxLines = 1)
    }
}

@Composable
private fun statusColor(session: MobileSession): Color = when {
    session.status.contains("fail", true) || session.status.contains("error", true) -> MaterialTheme.colorScheme.error
    session.status.contains("block", true) || session.status.contains("wait", true) -> Amber
    session.isProcessing || session.status in listOf("running", "working", "busy") -> Teal
    session.status in listOf("completed", "ready", "idle") -> Cobalt
    else -> Muted
}

@Composable
private fun SectionLabel(text: String) { Text(text.uppercase(), style = MaterialTheme.typography.labelSmall, color = Muted) }

@Composable
private fun SessionPanel(state: MobileState, selected: String?, onSelect: (MobileSession) -> Unit, onBroadcast: () -> Unit,
    query: String, onQuery: (String) -> Unit, activeOnly: Boolean, onActiveOnly: (Boolean) -> Unit) {
    val readStore = rememberThreadReadStore()
    val visible = remember(state.sessions, query, activeOnly) { visibleSessions(state.sessions, query, activeOnly) }
    Column(Modifier.fillMaxSize().background(MaterialTheme.colorScheme.surface)) {
        TextField(query, onQuery, modifier = Modifier.fillMaxWidth().padding(horizontal = 12.dp, vertical = 4.dp).testTag("session-search"),
            placeholder = { Text("Search sessions", style = MaterialTheme.typography.bodyMedium) }, singleLine = true,
            leadingIcon = { Icon(Icons.Outlined.Search, null, Modifier.size(20.dp)) },
            trailingIcon = { if (query.isNotEmpty()) IconButton(onClick = { onQuery("") }) { Icon(Icons.Outlined.Close, "Clear search", Modifier.size(18.dp)) } },
            shape = RoundedCornerShape(12.dp), textStyle = MaterialTheme.typography.bodyMedium,
            colors = TextFieldDefaults.colors(focusedContainerColor = Mist, unfocusedContainerColor = Mist, focusedIndicatorColor = Color.Transparent, unfocusedIndicatorColor = Color.Transparent))
        Row(Modifier.fillMaxWidth().padding(horizontal = 12.dp), verticalAlignment = Alignment.CenterVertically) {
            Row(Modifier.weight(1f).horizontalScroll(rememberScrollState()), horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                FilterChip(selected = !activeOnly, onClick = { onActiveOnly(false) }, label = { Text("All ${state.sessions.size}") })
                FilterChip(selected = activeOnly, onClick = { onActiveOnly(!activeOnly) }, label = { Text("Active ${state.sessions.count { it.isProcessing }}") })
            }
            IconButton(onClick = onBroadcast, enabled = state.sessions.isNotEmpty()) { Icon(Icons.Outlined.Campaign, "Message multiple sessions", Modifier.size(21.dp)) }
        }
        HorizontalDivider(color = Line)
        LazyColumn(Modifier.weight(1f).fillMaxWidth().testTag("session-list"), contentPadding = PaddingValues(vertical = 4.dp)) {
            if (visible.isEmpty()) item {
                EmptyPanel(if (state.sessions.isEmpty()) "No sessions yet" else "No matching sessions",
                    if (state.sessions.isEmpty()) "Start a session on your computer to see it here." else "Search by name, folder, or task. Try clearing your filters.")
                if (state.sessions.isNotEmpty()) TextButton(onClick = { onQuery(""); onActiveOnly(false) }, modifier = Modifier.padding(start = 16.dp)) { Text("Clear filters") }
            }
            items(visible, key = { it.id }) { session ->
                SessionCard(session, 0, session.id == selected, state.sessions.firstOrNull { it.id == session.parentId }?.name, unread = state.transcripts[session.id]?.count { readStore.unseen(state.readScope(), session.id, it) }, newActivity = readStore.hasActivity(state.readScope(), session.id)) { onSelect(session) }
            }
        }
    }
}

@Composable
private fun SessionCard(session: MobileSession, depth: Int, selected: Boolean, parentName: String? = null, unread: Int? = null, newActivity: Boolean = false, onClick: () -> Unit) {
    val color = statusColor(session)
    Surface(onClick = onClick, modifier = Modifier.fillMaxWidth().semantics { this.selected = selected }.testTag("session:${session.id}"),
        color = if (selected) MaterialTheme.colorScheme.primaryContainer else Color.Transparent) {
        Row(Modifier.fillMaxWidth().padding(horizontal = 10.dp, vertical = 6.dp), horizontalArrangement = Arrangement.spacedBy(10.dp), verticalAlignment = Alignment.Top) {
            Box(Modifier.padding(top = 2.dp).size(32.dp).background(color.copy(alpha = .09f), RoundedCornerShape(9.dp)), contentAlignment = Alignment.Center) {
                Icon(Icons.Outlined.Terminal, null, tint = color, modifier = Modifier.size(19.dp))
            }
            Column(Modifier.weight(1f), verticalArrangement = Arrangement.spacedBy(3.dp)) {
                Row(verticalAlignment = Alignment.CenterVertically) {
                    Text(session.name, Modifier.weight(1f), style = MaterialTheme.typography.titleMedium, maxLines = 1, overflow = TextOverflow.Ellipsis)
                    Spacer(Modifier.width(8.dp))
                    if (session.pinned) Icon(Icons.Outlined.PushPin, "Pinned session", Modifier.size(14.dp), tint = Cobalt)
                    Text(activityLabel(session.lastActivityAgeSecs), style = MaterialTheme.typography.labelSmall, color = Muted)
                }
                Text(session.workingDirectory.ifBlank { "Directory unavailable" }.let(::directorySummary), fontFamily = FontFamily.Monospace,
                    style = MaterialTheme.typography.bodySmall, color = Muted, maxLines = 1, overflow = TextOverflow.Ellipsis)
                val status = if (activelyWorking(session)) "Working" else session.status.replace('_', ' ').replaceFirstChar { it.uppercase() }
                val preview = session.currentTool.ifBlank { session.detail.ifBlank { session.taskLabel } }
                Text(if (preview.isBlank()) status else "$status · $preview", style = MaterialTheme.typography.bodySmall,
                    color = if (session.isProcessing) color else Muted, maxLines = 1, overflow = TextOverflow.Ellipsis)
                if (newActivity) Text("● New activity · open to refresh", color = Cobalt, style = MaterialTheme.typography.labelSmall, modifier = Modifier.testTag("session-activity-${session.id}"))
                if (unread != null && unread > 0) Text("$unread unread · loaded history", color = Cobalt, style = MaterialTheme.typography.labelSmall, modifier = Modifier.testTag("session-unread-${session.id}"))
                if (parentName != null) Text("↳ $parentName", style = MaterialTheme.typography.labelSmall, color = Muted, maxLines = 1, overflow = TextOverflow.Ellipsis)
            }
        }
    }
}

@Composable
private fun WorkingDirectory(path: String, compact: Boolean) {
    SelectionContainer {
        Row(Modifier.fillMaxWidth().padding(top = 7.dp), verticalAlignment = Alignment.Top) {
            Icon(Icons.Outlined.Folder, "Working directory", Modifier.padding(top = 2.dp).size(15.dp), tint = Muted)
            Spacer(Modifier.width(6.dp))
            Text(if (path.isBlank()) "Directory unavailable" else if (compact) directorySummary(path) else path,
                modifier = Modifier.weight(1f), style = MaterialTheme.typography.bodySmall,
                fontFamily = FontFamily.Monospace, color = Muted,
                maxLines = if (compact) 2 else Int.MAX_VALUE, overflow = TextOverflow.Ellipsis)
        }
    }
}

@Composable
private fun SessionDetail(state: MobileState, session: MobileSession?, tab: Int, onTab: (Int) -> Unit, draft: String, revision: Int,
    onDraft: (String) -> Unit, onSend: () -> Unit, onSelect: (MobileSession) -> Unit, onBack: (() -> Unit)?,
    showComposer: Boolean = true, threadState: LazyListState? = null, threadFollowLatest: MutableState<Boolean>? = null, onPin: () -> Unit = {}, onRename: (String) -> Unit = {}) {
    @Suppress("UNUSED_VARIABLE") val draftVersion = revision
    if (session == null) {
        Box(Modifier.fillMaxSize(), contentAlignment = Alignment.Center) { EmptyPanel("Choose a session", "Open a thread to follow its work and send a message.") }
        return
    }
    var menuOpen by remember(session.id) { mutableStateOf(false) }
    var notificationsOpen by rememberSaveable(session.id) { mutableStateOf(false) }
    var renameOpen by rememberSaveable(session.id) { mutableStateOf(false) }
    var customName by rememberSaveable(session.id) { mutableStateOf("") }
    val renameEditor: @Composable () -> Unit = {
        Column(Modifier.fillMaxWidth().background(MaterialTheme.colorScheme.surface).padding(12.dp), verticalArrangement = Arrangement.spacedBy(8.dp)) {
            Text("Display name on this phone. Leave blank to use the server name.", style = MaterialTheme.typography.bodySmall, color = Muted)
            OutlinedTextField(customName, { customName = it.take(80) }, label = { Text("Session name") }, singleLine = true, modifier = Modifier.fillMaxWidth().testTag("session-name-input"))
            Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.End) {
                TextButton(onClick = { renameOpen = false }) { Text("Cancel") }
                TextButton(onClick = { onRename(customName); renameOpen = false }) { Text("Save") }
            }
        }
    }
    if (notificationsOpen) AlertDialog(onDismissRequest = { notificationsOpen = false }, title = { Text("Session notifications") }, text = { SessionNotificationControls(state.host, session.id) }, confirmButton = { TextButton(onClick = { notificationsOpen = false }) { Text("Done") } })
    val agents = state.sessions.filter { it.parentId == session.id }
    Column(Modifier.fillMaxSize()) {
        Row(Modifier.fillMaxWidth().background(MaterialTheme.colorScheme.surface).heightIn(min = 52.dp).padding(start = if (onBack == null) 16.dp else 4.dp, end = 4.dp).testTag("thread-header"), verticalAlignment = Alignment.CenterVertically) {
            if (onBack != null) IconButton(onClick = { if (tab != 0) onTab(0) else onBack() }) {
                Icon(Icons.AutoMirrored.Outlined.ArrowBack, if (tab != 0) "Back to thread" else "Back to sessions")
            }
            Column(Modifier.weight(1f).clickable { onTab(if (tab == 0) 1 else 0) }.padding(vertical = 8.dp)) {
                Text(session.name, style = MaterialTheme.typography.titleMedium, maxLines = 1, overflow = TextOverflow.Ellipsis)
                val status = if (activelyWorking(session)) "Working" else session.status.replace('_', ' ').replaceFirstChar { it.uppercase() }
                Text("$status · ${directorySummary(session.workingDirectory.ifBlank { "No directory" })}", style = MaterialTheme.typography.bodySmall,
                    color = Muted, maxLines = 1, overflow = TextOverflow.Ellipsis)
            }
            IconButton(onClick = { onTab(if (tab == 0) 1 else 0) }) {
                Icon(if (tab == 0) Icons.Outlined.Info else Icons.Outlined.ChatBubbleOutline, if (tab == 0) "Session details" else "Return to thread", tint = Cobalt)
            }
            Box {
                IconButton(onClick = { menuOpen = true }) { Icon(Icons.Outlined.MoreVert, "Session actions") }
                DropdownMenu(expanded = menuOpen, onDismissRequest = { menuOpen = false }) {
                    DropdownMenuItem(text = { Text(if (session.pinned) "Unpin session" else "Pin session") }, leadingIcon = { Icon(Icons.Outlined.PushPin, null) }, onClick = { onPin(); menuOpen = false })
                    if (!state.isDemo && state.connection == ConnectionStatus.CONNECTED) DropdownMenuItem(text = { Text("Session notifications") }, leadingIcon = { Icon(Icons.Outlined.Notifications, null) }, onClick = { notificationsOpen = true; menuOpen = false })
                    DropdownMenuItem(text = { Text("Name session") }, leadingIcon = { Icon(Icons.Outlined.Edit, null) }, onClick = { customName = session.name; renameOpen = true; menuOpen = false })
                }
            }
        }
        HorizontalDivider(color = Line)
        if (tab == 0 && !renameOpen) ThreadParticipants(state, session, agents)
        if (renameOpen) renameEditor()
        if (tab != 0) TabRow(selectedTabIndex = (tab - 1).coerceIn(0, 1), containerColor = MaterialTheme.colorScheme.surface) {
            listOf("Work", "Agents (${agents.size})").forEachIndexed { index, title ->
                Tab(selected = tab == index + 1, onClick = { onTab(index + 1) }, text = { Text(title) })
            }
        }
        when (tab) {
            1 -> WorkPanel(session, Modifier.weight(1f))
            2 -> LazyColumn(Modifier.weight(1f).fillMaxWidth(), contentPadding = PaddingValues(vertical = 8.dp)) {
                if (agents.isEmpty()) item { EmptyPanel("No subagents", "Delegated agents and their work will appear here.") }
                items(agents.byRecentActivity(), key = { it.id }) { agent -> SessionCard(agent, 0, false) { onSelect(agent) } }
            }
            else -> key(session.id) { SessionThread(state, session, Modifier.weight(1f).fillMaxWidth().testTag("thread-viewport"), threadState, savedFollowLatest = threadFollowLatest, onQuote = { quote -> onDraft(draft + (if (draft.isBlank()) "" else "\n\n") + quote) }, readingEnabled = !renameOpen && !notificationsOpen && !menuOpen && LocalThreadReadingAllowed.current) }
        }
        if (tab == 0) AgentActivityIndicator(state, session)
        if (showComposer) SessionComposer(state, session, draft, onDraft, onSend)
    }
}

@Composable
private fun SessionComposer(state: MobileState, session: MobileSession, draft: String, onDraft: (String) -> Unit, onSend: () -> Unit) {
    val canSend = state.connection == ConnectionStatus.CONNECTED || state.isDemo
    Surface(color = MaterialTheme.colorScheme.surface, tonalElevation = 0.dp, modifier = Modifier.fillMaxWidth().testTag("composer")) {
        Column(Modifier.padding(horizontal = 8.dp, vertical = 4.dp)) {
            if (canSend) Text("To ${session.agentName.ifBlank { session.name }} · this session only", style = MaterialTheme.typography.labelSmall, color = Muted, maxLines = 1, overflow = TextOverflow.Ellipsis, modifier = Modifier.padding(bottom = 4.dp))
            if (!canSend) Text("Offline · reconnect to send", style = MaterialTheme.typography.bodySmall, color = Amber, modifier = Modifier.padding(bottom = 4.dp))
            val recoverable = state.deliveries.lastOrNull { it.sessionId == session.id }
                ?.takeIf { it.status == DeliveryStatus.REJECTED || it.status == DeliveryStatus.UNKNOWN }
            var restoredId by rememberSaveable(session.id) { mutableStateOf<Long?>(null) }
            if (recoverable != null && recoverable.requestId != restoredId) {
                Text(if (recoverable.status == DeliveryStatus.UNKNOWN) "Delivery unconfirmed. Check before resending." else "Message was not sent.", style = MaterialTheme.typography.labelSmall, color = Amber)
                TextButton(onClick = {
                    onDraft(if (draft.isBlank()) recoverable.text else "$draft\n\n${recoverable.text}")
                    restoredId = recoverable.requestId
                }) { Text("Restore draft") }
            }
            Row(verticalAlignment = Alignment.Bottom, horizontalArrangement = Arrangement.spacedBy(4.dp)) {
                QuickMessages(draft, onDraft, compact = true)
                TextField(value = draft, onValueChange = onDraft, modifier = Modifier.weight(1f).testTag("message-input"),
                    placeholder = { Text("Message ${session.agentName.ifBlank { session.name }}…", maxLines = 1, overflow = TextOverflow.Ellipsis) }, maxLines = 4, shape = RoundedCornerShape(20.dp),
                    textStyle = MaterialTheme.typography.bodyLarge,
                    colors = TextFieldDefaults.colors(focusedContainerColor = Mist, unfocusedContainerColor = Mist, focusedIndicatorColor = Color.Transparent, unfocusedIndicatorColor = Color.Transparent))
                FilledIconButton(onClick = onSend, enabled = canSend && draft.isNotBlank(), modifier = Modifier.padding(bottom = 4.dp).size(48.dp)) {
                    Icon(Icons.AutoMirrored.Outlined.Send, "Send message to ${session.name}")
                }
            }
            if (state.isDemo) Text("Demo · messages stay on this device", style = MaterialTheme.typography.bodySmall, color = Muted, modifier = Modifier.padding(top = 4.dp))
        }
    }
}

@Composable
internal fun SessionThread(state: MobileState, session: MobileSession, modifier: Modifier = Modifier, savedListState: LazyListState? = null, onQuote: ((String) -> Unit)? = null, readingEnabled: Boolean = true, savedFollowLatest: MutableState<Boolean>? = null) {
    val rows = threadRowsNewestFirst(session.copy(isProcessing = activelyWorking(session) && (state.connection == ConnectionStatus.CONNECTED || state.isDemo)), state.transcripts[session.id].orEmpty(), state.deliveries)
    val listState = savedListState ?: rememberLazyListState()
    val localFollowLatest = remember(listState) { mutableStateOf(listState.firstVisibleItemIndex == 0 && listState.firstVisibleItemScrollOffset == 0) }
    var followLatest by (savedFollowLatest ?: localFollowLatest)
    var autoScrolling by remember { mutableStateOf(false) }
    val scope = rememberCoroutineScope()
    val readStore = rememberThreadReadStore()
    val host = state.readScope()
    val lifecycleOwner = LocalLifecycleOwner.current
    var searchOpen by rememberSaveable(session.id) { mutableStateOf(false) }
    var highlighted by rememberSaveable(session.id) { mutableStateOf<String?>(null) }
    val unreadRows = rows.filterIsInstance<ThreadRow.Message>().filter { !it.live && readStore.unseen(host, session.id, it.entry) }
    val firstUnread = unreadRows.lastOrNull()?.key
    // Freeze this visit's boundary, so acknowledgements never move it under the reader.
    var visitBoundary by rememberSaveable(host, session.id) { mutableStateOf<String?>(null) }
    LaunchedEffect(firstUnread) { if (visitBoundary == null && firstUnread != null) visitBoundary = firstUnread }
    val jump: (String) -> Unit = { rowKey ->
        val index = rows.indexOfFirst { it.key == rowKey }
        if (index >= 0) {
            followLatest = false
            highlighted = rowKey
            scope.launch {
                autoScrolling = true
                try {
                    // Allow selection/boundary content to measure before positioning the target.
                    withFrameNanos { }
                    listState.scrollToItem(index)
                    withFrameNanos { }
                    val layout = listState.layoutInfo
                    val target = layout.visibleItemsInfo.firstOrNull { it.key == rowKey }
                    if (target != null) {
                        // reverseLayout's default aligns the END of the card. Search/catch-up
                        // needs its heading and beginning, including for cards taller than the viewport.
                        val viewport = layout.viewportEndOffset - layout.viewportStartOffset
                        listState.scrollToItem(index, target.size - viewport + layout.beforeContentPadding)
                    }
                } finally { autoScrolling = false }
            }
        }
    }
    LaunchedEffect(listState, rows, host, session.id, catchUpActivityKey(session), readingEnabled, searchOpen, lifecycleOwner) {
        if (readingEnabled && !searchOpen) lifecycleOwner.lifecycle.repeatOnLifecycle(Lifecycle.State.RESUMED) {
            snapshotFlow {
                val layout = listState.layoutInfo
                if (listState.isScrollInProgress) emptyList() else layout.visibleItemsInfo.filter { item ->
                    val visible = minOf(item.offset + item.size, layout.viewportEndOffset) - maxOf(item.offset, layout.viewportStartOffset)
                    visible > 0 && visible >= minOf(item.size, layout.viewportEndOffset - layout.viewportStartOffset) / 2
                }.map { it.key.toString() }
            }.distinctUntilChanged().collectLatest { keys ->
                // A brief stable, foreground viewport is evidence. Polling and off-screen rows are not.
                delay(700)
                readStore.acknowledge(host, session.id, rows.filterIsInstance<ThreadRow.Message>()
                    .filter { !it.live && it.key in keys }.map { it.entry })
                if (rows.firstOrNull()?.key in keys) readStore.viewLatestActivity(host, session.id)
            }
        }
    }
    if (searchOpen) ThreadSearchDialog(state.transcripts[session.id].orEmpty(), session.agentName.ifBlank { session.name },
        onDismiss = { searchOpen = false }, onJump = { entryId ->
            val entry = state.transcripts[session.id].orEmpty().firstOrNull { it.id == entryId }
            val row = rows.filterIsInstance<ThreadRow.Message>().firstOrNull { !it.live &&
                (it.entry.id == entryId || (entry?.messageId != null && it.entry.messageId == entry.messageId)) }
            if (row != null) { searchOpen = false; jump(row.key) }
        })

    LaunchedEffect(listState) {
        // Observe actual scroll gestures/flings, not index shifts caused by new rows.
        // The first idle emission is layout state, not a completed user gesture.
        // On remount, key anchoring can temporarily place the old newest row at index 1.
        snapshotFlow { listState.isScrollInProgress }.drop(1).collect { scrolling ->
            followLatest = followLatestAfterScroll(autoScrolling, scrolling,
                listState.firstVisibleItemIndex == 0 && listState.firstVisibleItemScrollOffset == 0, followLatest)
        }
    }
    LaunchedEffect(rows, followLatest) {
        if (followLatest && !listState.isScrollInProgress) {
            autoScrolling = true
            try {
                // Let the remounted list apply its new data/key anchors before restoring intent.
                withFrameNanos { }
                listState.scrollToItem(0)
            } finally { autoScrolling = false }
        }
    }
    Box(modifier) {
        Column(Modifier.fillMaxSize()) {
        Row(Modifier.fillMaxWidth().padding(horizontal = 8.dp), verticalAlignment = Alignment.CenterVertically) {
            TextButton(onClick = { firstUnread?.let(jump) }, enabled = firstUnread != null,
                modifier = Modifier.weight(1f).testTag("catch-up-unread")) {
                Text(if (session.id !in state.transcripts) "History not loaded" else if (unreadRows.isEmpty()) "Caught up · loaded history" else "${unreadRows.size} unread · loaded history", maxLines = 1, overflow = TextOverflow.Ellipsis)
            }
            IconButton(onClick = { searchOpen = true }, modifier = Modifier.testTag("thread-search-open")) { Icon(Icons.Outlined.Search, "Search messages in this session") }
        }
        // Reversed DATA + reversed LAYOUT retains oldest-to-newest visual order,
        // while index zero anchors the viewport to the END of the newest card.
        LazyColumn(Modifier.weight(1f).fillMaxWidth().testTag("thread-message-list"), state = listState, reverseLayout = true,
            contentPadding = PaddingValues(horizontal = 8.dp, vertical = 6.dp), verticalArrangement = Arrangement.spacedBy(4.dp)) {
            items(rows, key = { it.key }) { row ->
                when (row) {
                    is ThreadRow.Message -> Box(Modifier.fillMaxWidth(), contentAlignment = Alignment.Center) { Column(Modifier.widthIn(max = 880.dp).fillMaxWidth()) {
                        if (row.key == visitBoundary) Text("First unread on this visit", color = Cobalt,
                            style = MaterialTheme.typography.labelMedium, modifier = Modifier.testTag("unread-boundary").padding(bottom = 8.dp))
                        if (row.key == highlighted) Text("Selected message", color = Cobalt, style = MaterialTheme.typography.labelSmall,
                            modifier = Modifier.testTag("search-highlight"))
                        if (row.live) {
                            SectionLabel("Latest activity")
                            Spacer(Modifier.height(8.dp))
                        }
                        TranscriptCard(row.entry, session.agentName.ifBlank { session.name }, onQuote, row.receipt)
                    } }
                    ThreadRow.Empty -> EmptyPanel("The thread starts here", if (session.isProcessing)
                        "This session is working. Output will appear as it arrives." else
                        "Send an instruction below, or review this session’s work and agents.")
                }
            }
        }
        }
        if (!followLatest) SmallFloatingActionButton(
            onClick = {
                scope.launch {
                    autoScrolling = true
                    try {
                        listState.animateScrollToItem(0)
                        followLatest = true
                    } finally { autoScrolling = false }
                }
            }, modifier = Modifier.align(Alignment.BottomEnd).padding(12.dp), containerColor = Cobalt,
            contentColor = MaterialTheme.colorScheme.surface) {
            Icon(Icons.Outlined.ArrowDownward, "Jump to latest messages")
        }
    }
}

@Composable
private fun TranscriptCard(entry: TranscriptEntry, agentName: String = "Jcode", onQuote: ((String) -> Unit)? = null, receipt: MessageReceipt? = null) {
    val attribution = messageAttribution(entry)
    val isUser = attribution.isHuman
    if (attribution.isEvent) { ThreadEventCard(entry, attribution, onQuote); return }
    val sender = attribution.sender ?: if (entry.role == "assistant") agentName else attribution.label
    Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.spacedBy(8.dp), verticalAlignment = Alignment.Top) {
    if (!isUser) SenderAvatar(sender, Modifier.padding(top = 8.dp))
    Box(Modifier.fillMaxWidth(), contentAlignment = if (isUser) Alignment.CenterEnd else Alignment.CenterStart) {
    Surface(modifier = Modifier.fillMaxWidth(if (isUser) 0.9f else 1f), shape = RoundedCornerShape(12.dp), color = if (isUser) MaterialTheme.colorScheme.primaryContainer else Color.Transparent) {
        Column(Modifier.fillMaxWidth().padding(horizontal = 8.dp, vertical = 4.dp), verticalArrangement = Arrangement.spacedBy(2.dp)) {
            Row(Modifier.fillMaxWidth(), verticalAlignment = Alignment.CenterVertically) {
                Text(sender, Modifier.weight(1f), maxLines = 1, overflow = TextOverflow.Ellipsis, style = MaterialTheme.typography.labelLarge, fontWeight = FontWeight.SemiBold, color = if (isUser) Muted else senderColor(sender))
                if (attribution.sender != null) Text(if (attribution.label.startsWith("DM")) " · direct message" else " · ${attribution.label.substringBefore(" ·")}", style = MaterialTheme.typography.labelSmall, color = Muted)
                MessageTimestamp(entry.timestampUnixMs)
                MessageActions(sender, attribution.body ?: entry.text, onQuote)
            }
            if (entry.text.isNotBlank()) MarkdownMessage(attribution.body ?: entry.text)
            if (isUser && receipt != null) MessageReceiptView(receipt)
            if (entry.reasoning.isNotBlank()) {
                var reasoningOpen by rememberSaveable(entry.id) { mutableStateOf(false) }
                TextButton(onClick = { reasoningOpen = !reasoningOpen }) { Text(if (reasoningOpen) "Hide reasoning" else "Show reasoning") }
                if (reasoningOpen) MarkdownMessage(entry.reasoning, style = MaterialTheme.typography.bodyMedium.copy(color = Muted))
            }
            entry.tools.forEach { tool ->
                var open by rememberSaveable(tool.id) { mutableStateOf(false) }
                Surface(onClick = { open = !open }, shape = RoundedCornerShape(10.dp), color = Mist) {
                    Column(Modifier.fillMaxWidth().padding(8.dp)) {
                        Row(verticalAlignment = Alignment.CenterVertically) {
                            Icon(Icons.Outlined.Terminal, null, Modifier.size(18.dp), tint = Cobalt)
                            Text(tool.name, Modifier.weight(1f).padding(start = 8.dp), style = MaterialTheme.typography.labelLarge)
                            Text(tool.status, style = MaterialTheme.typography.labelSmall, color = Muted)
                            Icon(if (open) Icons.Outlined.ExpandLess else Icons.Outlined.ExpandMore, if (open) "Collapse tool output" else "Expand tool output")
                        }
                        if (open) {
                            if (tool.input.isNotBlank()) MarkdownCode(tool.input, "input")
                            MarkdownCode(tool.error ?: tool.output.ifBlank { "No output yet" }, if (tool.error != null) "error" else "output")
                        }
                    }
                }
            }
        }
    }
}
}
}

@Composable
private fun WorkPanel(session: MobileSession, modifier: Modifier) {
    LazyColumn(modifier.fillMaxWidth(), contentPadding = PaddingValues(12.dp), verticalArrangement = Arrangement.spacedBy(8.dp)) {
        item {
            SectionLabel("Working directory")
            WorkingDirectory(session.workingDirectory, compact = false)
        }
        item {
            SectionLabel("CURRENT FOCUS")
            Text(session.detail.ifBlank { session.taskLabel.ifBlank { "No work summary available yet." } }, style = MaterialTheme.typography.titleLarge, modifier = Modifier.padding(top = 8.dp))
            if (session.model.isNotBlank()) Text(session.model, style = MaterialTheme.typography.labelSmall, color = Muted, modifier = Modifier.padding(top = 10.dp))
        }
        item { HorizontalDivider(color = Line); Spacer(Modifier.height(8.dp)); SectionLabel("TASKS · ${session.todosCompleted}/${session.todosTotal}") }
        if (session.todos.isEmpty()) item { Text("This session has not published a task list yet.", color = Muted) }
        items(session.todos) { task ->
            val done = task.status == "completed"
            val active = task.status == "in_progress"
            Row(Modifier.fillMaxWidth().background(MaterialTheme.colorScheme.surface, RoundedCornerShape(12.dp)).padding(8.dp), horizontalArrangement = Arrangement.spacedBy(12.dp)) {
                Icon(if (done) Icons.Outlined.CheckCircle else if (active) Icons.Outlined.PlayCircle else Icons.Outlined.RadioButtonUnchecked, contentDescription = task.status.replace('_', ' '), tint = if (done) Teal else if (active) Cobalt else Muted, modifier = Modifier.size(22.dp))
                Column { Text(task.content, style = MaterialTheme.typography.bodyMedium); Text(task.status.replace('_', ' '), style = MaterialTheme.typography.labelSmall, color = Muted, modifier = Modifier.padding(top = 4.dp)) }
            }
        }
        if (session.completionReport.isNotBlank()) item { SectionLabel("COMPLETION REPORT"); MarkdownMessage(session.completionReport, Modifier.padding(top = 8.dp)) }
    }
}

@Composable
private fun DeliveryRow(result: DeliveryResult) {
    val color = when (result.status) { DeliveryStatus.ACCEPTED -> Teal; DeliveryStatus.REJECTED -> MaterialTheme.colorScheme.error; else -> Amber }
    Row(Modifier.fillMaxWidth().background(color.copy(alpha = .07f), RoundedCornerShape(12.dp)).padding(12.dp), horizontalArrangement = Arrangement.spacedBy(10.dp)) {
        Icon(if (result.status == DeliveryStatus.ACCEPTED) Icons.Outlined.CheckCircle else Icons.Outlined.Info, null, tint = color, modifier = Modifier.size(20.dp))
        Column {
            Text("${result.sessionName} · ${when (result.status) { DeliveryStatus.ACCEPTED -> "Sent"; DeliveryStatus.SENDING -> "Sending…"; DeliveryStatus.REJECTED -> "Not sent"; DeliveryStatus.UNKNOWN -> "Unconfirmed" }}", style = MaterialTheme.typography.labelLarge, color = color)
            Text(result.text, style = MaterialTheme.typography.bodyMedium, maxLines = 2, overflow = TextOverflow.Ellipsis)
            if (result.detail.isNotBlank()) Text(result.detail, style = MaterialTheme.typography.bodySmall, color = Muted)
        }
    }
}

@Composable
private fun EmptyPanel(title: String, detail: String) {
    Column(Modifier.padding(12.dp), verticalArrangement = Arrangement.spacedBy(6.dp)) {
        Icon(Icons.Outlined.AccountTree, null, tint = Cobalt, modifier = Modifier.size(30.dp))
        Text(title, style = MaterialTheme.typography.titleLarge)
        Text(detail, style = MaterialTheme.typography.bodyMedium, color = Muted)
    }
}

@Composable
private fun WelcomePanel(state: MobileState, onPair: () -> Unit, onDemo: () -> Unit) {
    Column(Modifier.fillMaxSize().verticalScroll(rememberScrollState()).padding(16.dp), verticalArrangement = Arrangement.spacedBy(12.dp)) {
        Spacer(Modifier.height(12.dp))
        SectionLabel("JCODE / MOBILE")
        Text("Your agents.\nOne clear view.", style = MaterialTheme.typography.headlineLarge)
        Text("Stay close to the work, wherever you are. Follow live sessions, inspect agent outputs, and send the next instruction.", color = Muted, style = MaterialTheme.typography.bodyLarge)
        Surface(shape = RoundedCornerShape(20.dp), color = MaterialTheme.colorScheme.surface) {
            Column(Modifier.padding(12.dp), verticalArrangement = Arrangement.spacedBy(10.dp)) {
                FeatureRow(Icons.Outlined.AccountTree, "Follow every thread", "Sessions and subagents, connected by their actual work.")
                FeatureRow(Icons.Outlined.CheckCircle, "See what’s moving", "Live status, task lists, and tool output.")
                FeatureRow(Icons.Outlined.Campaign, "Keep work aligned", "Message one session or confirm a broadcast.")
            }
        }
        Button(onClick = onPair, modifier = Modifier.fillMaxWidth().heightIn(min = 54.dp)) { Text("Connect your computer") }
        OutlinedButton(onClick = onDemo, modifier = Modifier.fillMaxWidth().heightIn(min = 52.dp)) { Text("Preview demo") }
        Text("Demo is a local preview. It never connects to your sessions.", style = MaterialTheme.typography.bodySmall, color = Muted)
    }
}

@Composable
private fun FeatureRow(icon: ImageVector, title: String, description: String) {
    Row(horizontalArrangement = Arrangement.spacedBy(14.dp)) {
        Icon(icon, null, tint = Cobalt, modifier = Modifier.size(23.dp))
        Column { Text(title, style = MaterialTheme.typography.titleMedium); Text(description, style = MaterialTheme.typography.bodyMedium, color = Muted) }
    }
}

@Composable
private fun SettingsPanel(state: MobileState, vm: MobileViewModel) {
    var host by rememberSaveable(state.host) { mutableStateOf(state.host) }
    // Pairing codes deliberately are not persisted across process recreation.
    var code by remember { mutableStateOf("") }
    val pairing = state.connection == ConnectionStatus.PAIRING
    val appearance by vm.appearance.collectAsState()
    LazyColumn(Modifier.fillMaxSize(), contentPadding = PaddingValues(12.dp), verticalArrangement = Arrangement.spacedBy(12.dp)) {
        item {
            SectionLabel("APPEARANCE")
            Text("Theme", style = MaterialTheme.typography.titleLarge, modifier = Modifier.padding(top = 8.dp))
            ThemeMode.entries.forEach { mode ->
                Row(Modifier.fillMaxWidth().clickable { vm.setTheme(mode) }.heightIn(min = 48.dp), verticalAlignment = Alignment.CenterVertically) {
                    RadioButton(selected = appearance.theme == mode, onClick = { vm.setTheme(mode) })
                    Text(when (mode) { ThemeMode.SYSTEM -> "Use device setting"; ThemeMode.LIGHT -> "Light"; ThemeMode.DARK -> "Dark" })
                }
            }
        }
        item { SectionLabel("CONNECTION"); Text("Your workspace", style = MaterialTheme.typography.headlineLarge, modifier = Modifier.padding(top = 8.dp)) }
        item {
            Surface(shape = RoundedCornerShape(18.dp), color = MaterialTheme.colorScheme.surface) {
                Column(Modifier.padding(12.dp), verticalArrangement = Arrangement.spacedBy(8.dp)) {
                    Text("Pair with your computer", style = MaterialTheme.typography.titleLarge)
                    Text("Start the Jcode mobile gateway on your computer, then enter its hostname and the six-digit pairing code shown there.", style = MaterialTheme.typography.bodyMedium, color = Muted)
                    OutlinedTextField(host, { host = it }, label = { Text("Hostname or gateway URL") }, placeholder = { Text("100.64.0.2:7643") }, singleLine = true, modifier = Modifier.fillMaxWidth(), keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Uri), enabled = !pairing)
                    OutlinedTextField(code, { code = it.filter { char -> char in '0'..'9' }.take(6) }, label = { Text("Six-digit pairing code") }, singleLine = true, modifier = Modifier.fillMaxWidth(), keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.NumberPassword), visualTransformation = PasswordVisualTransformation(), enabled = !pairing)
                    Text("Only connect over HTTPS, Tailscale, or a trusted private network. Plain HTTP is not encrypted.", style = MaterialTheme.typography.bodySmall, color = Amber)
                    Button(onClick = { vm.pair(host.trim(), code); code = "" }, enabled = host.isNotBlank() && code.length == 6 && !pairing, modifier = Modifier.fillMaxWidth().heightIn(min = 52.dp)) {
                        if (pairing) { CircularProgressIndicator(Modifier.size(18.dp), strokeWidth = 2.dp); Spacer(Modifier.width(10.dp)) }
                        Text(if (pairing) "Pairing…" else "Pair and connect")
                    }
                }
            }
        }
        item {
            Surface(shape = RoundedCornerShape(16.dp), color = Amber.copy(alpha = .09f)) {
                Column(Modifier.padding(12.dp), verticalArrangement = Arrangement.spacedBy(6.dp)) {
                    Row(horizontalArrangement = Arrangement.spacedBy(10.dp)) { Icon(Icons.Outlined.Security, null, tint = Amber); Text("Use a trusted network", style = MaterialTheme.typography.titleMedium, color = Amber) }
                    Text("Plain HTTP / WebSocket connections are not encrypted. Use Tailscale, a trusted private network, or an HTTPS gateway. Never expose the gateway directly to the public internet.", style = MaterialTheme.typography.bodyMedium)
                }
            }
        }
        item {
            SectionLabel("CURRENT CONNECTION")
            Text(if (state.isDemo) "Demo preview · no server connection" else state.host.ifBlank { "Not paired" }, style = MaterialTheme.typography.bodyLarge, modifier = Modifier.padding(top = 8.dp))
            if (!state.isDemo && state.host.isNotBlank()) Text("Forgetting pairing removes this device’s saved access. You will need a new six-digit code to reconnect.", style = MaterialTheme.typography.bodySmall, color = Muted, modifier = Modifier.padding(top = 8.dp))
            Row(horizontalArrangement = Arrangement.spacedBy(10.dp)) {
                TextButton(onClick = vm::refresh, enabled = state.host.isNotBlank() && !state.isDemo) { Text("Reconnect") }
                TextButton(onClick = vm::disconnect, enabled = state.host.isNotBlank() || state.isDemo) { Text(if (state.isDemo) "Leave demo" else "Forget pairing") }
            }
        }
        item { OutlinedButton(onClick = vm::loadDemo, modifier = Modifier.fillMaxWidth().heightIn(min = 52.dp)) { Text("Preview demo") }; Text("Local sample sessions. Your computer is not contacted.", color = Muted, style = MaterialTheme.typography.bodySmall, modifier = Modifier.padding(top = 8.dp)) }
    }
}

@Composable
private fun BroadcastDialog(state: MobileState, onDismiss: () -> Unit, onSend: (List<String>, String) -> Unit) {
    var text by rememberSaveable { mutableStateOf("") }
    var selectedIds by rememberSaveable { mutableStateOf(state.sessions.map { it.id }) }
    val recipients = state.sessions.filter { it.id in selectedIds }
    val canSend = state.connection == ConnectionStatus.CONNECTED || state.isDemo
    AlertDialog(
        onDismissRequest = onDismiss,
        icon = { Icon(Icons.Outlined.Campaign, null, tint = Cobalt) },
        title = { Text("Message sessions") },
        text = {
            Column(Modifier.heightIn(max = 460.dp).verticalScroll(rememberScrollState()), verticalArrangement = Arrangement.spacedBy(12.dp)) {
                Text("Each selected session will receive this instruction. Review the recipients before sending.")
                if (state.isDemo) StatusPill("Demo · local only", Cobalt)
                state.sessions.forEach { session ->
                    Row(Modifier.fillMaxWidth().heightIn(min = 48.dp).clickable {
                        selectedIds = if (session.id in selectedIds) selectedIds - session.id else selectedIds + session.id
                    }, verticalAlignment = Alignment.CenterVertically) {
                        Checkbox(checked = session.id in selectedIds, onCheckedChange = { checked -> selectedIds = if (checked) selectedIds + session.id else selectedIds - session.id })
                        Column(Modifier.weight(1f)) { Text(session.name, style = MaterialTheme.typography.titleMedium); Text(session.status, style = MaterialTheme.typography.labelSmall, color = statusColor(session)) }
                    }
                }
                OutlinedTextField(value = text, onValueChange = { text = it }, label = { Text("Instruction") }, minLines = 3, maxLines = 6, modifier = Modifier.fillMaxWidth())
                Text("${recipients.size} recipient${if (recipients.size == 1) "" else "s"}: ${recipients.joinToString { it.name }}", style = MaterialTheme.typography.bodySmall, color = Muted)
                if (!canSend) Text("Reconnect before sending.", color = Amber)
            }
        },
        confirmButton = { Button(onClick = { onSend(recipients.map { it.id }, text.trim()) }, enabled = recipients.isNotEmpty() && text.isNotBlank() && canSend) { Text("Confirm send (${recipients.size})") } },
        dismissButton = { TextButton(onClick = onDismiss) { Text("Cancel") } }
    )
}
