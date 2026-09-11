package dev.jcode.mobile.ui

import androidx.compose.animation.core.*
import androidx.compose.foundation.background
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.outlined.*
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.alpha
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.unit.dp
import dev.jcode.mobile.data.*

internal fun activelyWorking(session: MobileSession): Boolean = session.isProcessing &&
    session.status.lowercase() !in setOf("idle", "ready", "waiting", "waiting_for_input", "waiting_for_user", "awaiting_input", "completed", "done", "stopped", "failed", "paused")

@Composable
internal fun MessageReceiptView(receipt: MessageReceipt) {
    val color = when (receipt) {
        MessageReceipt.READ -> Cobalt
        MessageReceipt.FAILED -> MaterialTheme.colorScheme.error
        MessageReceipt.UNKNOWN -> Amber
        else -> Muted
    }
    Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.End, verticalAlignment = Alignment.CenterVertically) {
        Icon(when (receipt) {
            MessageReceipt.READ -> Icons.Outlined.DoneAll
            MessageReceipt.SENT -> Icons.Outlined.Done
            MessageReceipt.SENDING -> Icons.Outlined.Schedule
            MessageReceipt.FAILED -> Icons.Outlined.ErrorOutline
            MessageReceipt.UNKNOWN -> Icons.Outlined.HelpOutline
        }, null, Modifier.size(14.dp), tint = color)
        Spacer(Modifier.width(4.dp))
        Text(receipt.label, color = color, style = MaterialTheme.typography.labelSmall)
    }
}

@Composable
internal fun AgentActivityIndicator(state: MobileState, session: MobileSession) {
    if (state.connection != ConnectionStatus.CONNECTED && !state.isDemo) return
    val active = (listOf(session) + state.sessions.filter { it.parentId == session.id }).distinctBy { it.id }.filter(::activelyWorking)
    if (active.isEmpty()) return
    val animation = rememberInfiniteTransition(label = "Agent activity")
    val pulse by animation.animateFloat(.35f, 1f, infiniteRepeatable(tween(850), RepeatMode.Reverse), label = "Activity dots")
    Column(Modifier.fillMaxWidth().padding(horizontal = 18.dp, vertical = 6.dp).testTag("agent-activity"), verticalArrangement = Arrangement.spacedBy(5.dp)) {
        active.take(3).forEach { agent ->
            val name = agent.agentName.ifBlank { agent.name }
            val label = if (agent.currentTool.isNotBlank()) "$name is using ${agent.currentTool}…" else "$name is working…"
            Row(Modifier.semantics(mergeDescendants = true) { contentDescription = label }, verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                Row(horizontalArrangement = Arrangement.spacedBy(3.dp)) {
                    repeat(3) { dot -> Box(Modifier.size(4.dp).alpha(if (dot == 1) 1.35f - pulse else pulse).background(Teal, CircleShape)) }
                }
                Text(label, color = Muted, style = MaterialTheme.typography.bodySmall)
            }
        }
        if (active.size > 3) Text("${active.size - 3} more agents working…", color = Muted, style = MaterialTheme.typography.labelSmall)
    }
}
