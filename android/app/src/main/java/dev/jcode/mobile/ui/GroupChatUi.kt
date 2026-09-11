package dev.jcode.mobile.ui

import androidx.compose.foundation.background
import androidx.compose.foundation.horizontalScroll
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.outlined.*
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import dev.jcode.mobile.data.*

@Composable
internal fun senderColor(name: String): Color {
    val colors = listOf(Cobalt, Teal, Amber, MaterialTheme.colorScheme.onSurfaceVariant)
    return colors[(name.lowercase().hashCode() and Int.MAX_VALUE) % colors.size]
}

@Composable
internal fun SenderAvatar(name: String, modifier: Modifier = Modifier, small: Boolean = false) {
    val color = senderColor(name)
    Box(modifier.size(if (small) 22.dp else 30.dp).background(color.copy(alpha = .15f), CircleShape), contentAlignment = Alignment.Center) {
        Text(name.trim().take(1).uppercase().ifBlank { "?" }, color = color, style = MaterialTheme.typography.labelMedium, fontWeight = FontWeight.Bold)
    }
}

@Composable
internal fun ThreadParticipants(state: MobileState, session: MobileSession, children: List<MobileSession>) {
    val agent = session.agentName.ifBlank { session.name }
    val senders = state.transcripts[session.id].orEmpty().mapNotNull { messageAttribution(it).sender }
    val names = (listOf("You", agent) + children.map { it.agentName.ifBlank { it.name } } + senders).distinct()
    Row(Modifier.fillMaxWidth().background(MaterialTheme.colorScheme.surface).horizontalScroll(rememberScrollState()).padding(horizontal = 12.dp, vertical = 7.dp),
        horizontalArrangement = Arrangement.spacedBy(12.dp), verticalAlignment = Alignment.CenterVertically) {
        names.forEach { name ->
            Row(horizontalArrangement = Arrangement.spacedBy(5.dp), verticalAlignment = Alignment.CenterVertically) {
                SenderAvatar(name, small = true)
                Text(name, style = MaterialTheme.typography.labelMedium, color = Muted, maxLines = 1)
            }
        }
        if (children.isNotEmpty()) Text("${children.size} subagents · see Details", style = MaterialTheme.typography.labelSmall, color = Muted)
    }
}

@Composable
internal fun ThreadEventCard(entry: TranscriptEntry, attribution: MessageAttribution, onQuote: ((String) -> Unit)? = null) {
    var expanded by rememberSaveable(entry.id) { mutableStateOf(false) }
    val background = attribution.label.startsWith("Background") || attribution.label == "Shell output"
    Surface(shape = RoundedCornerShape(12.dp), color = MaterialTheme.colorScheme.surface, modifier = Modifier.fillMaxWidth()) {
        Column(Modifier.padding(horizontal = 12.dp, vertical = 6.dp)) {
            Row(Modifier.fillMaxWidth(), verticalAlignment = Alignment.CenterVertically) {
                Icon(if (background) Icons.Outlined.Terminal else Icons.Outlined.Info, null, Modifier.size(18.dp), tint = Muted)
                Text(attribution.label, Modifier.weight(1f).padding(start = 8.dp), style = MaterialTheme.typography.labelLarge, color = Muted)
                MessageActions(attribution.label, entry.text, onQuote)
                IconButton(onClick = { expanded = !expanded }, modifier = Modifier.size(48.dp)) {
                    Icon(if (expanded) Icons.Outlined.ExpandLess else Icons.Outlined.ExpandMore, if (expanded) "Hide ${attribution.label} details" else "Show ${attribution.label} details", tint = Muted)
                }
            }
            MessageTimestamp(entry.timestampUnixMs)
            if (expanded) {
                MarkdownMessage(entry.text, Modifier.padding(bottom = 8.dp), MaterialTheme.typography.bodyMedium)
            } else {
                val summary = entry.text.trimStart().lineSequence().firstOrNull().orEmpty()
                    .replace(Regex("^\\*\\*[^*]+\\*\\*\\s*"), "").replace("`", "")
                Text(if (attribution.label == "Automation") "Automatic progress check" else summary,
                    style = MaterialTheme.typography.bodySmall, color = Muted, maxLines = 2, overflow = TextOverflow.Ellipsis, modifier = Modifier.padding(bottom = 6.dp))
            }
        }
    }
}
