package dev.jcode.mobile.ui

import androidx.compose.foundation.layout.*
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.outlined.Refresh
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.unit.dp
import dev.jcode.mobile.data.*
import java.time.Instant
import java.time.ZoneId
import java.time.format.DateTimeFormatter
import kotlin.math.roundToInt

private fun usageTime(value: String): String = runCatching {
    DateTimeFormatter.ofPattern("MMM d, HH:mm z").withZone(ZoneId.systemDefault()).format(Instant.parse(value))
}.getOrDefault(value)

@Composable
internal fun ProviderUsagePage(usage: ProviderUsageState, state: MobileState, onRefresh: () -> Unit) {
    LaunchedEffect(Unit) { onRefresh() }
    LazyColumn(Modifier.fillMaxSize().testTag("provider-usage-page"), contentPadding = PaddingValues(12.dp), verticalArrangement = Arrangement.spacedBy(8.dp)) {
        item {
            Row(Modifier.fillMaxWidth(), verticalAlignment = Alignment.CenterVertically) {
                Column(Modifier.weight(1f)) {
                    Text("Model allowances", style = MaterialTheme.typography.headlineSmall)
                    Text("Configured providers on your computer", color = Muted, style = MaterialTheme.typography.bodySmall)
                }
                IconButton(onClick = onRefresh, enabled = !usage.loading && state.connection == ConnectionStatus.CONNECTED) { Icon(Icons.Outlined.Refresh, "Refresh provider usage") }
            }
            if (usage.loading) LinearProgressIndicator(Modifier.fillMaxWidth().padding(top = 10.dp))
            usage.fetchedAtSeconds?.let {
                Text("${if (usage.fromCache) "Cached" else "Checked"} · ${usageTime(Instant.ofEpochSecond(it).toString())}", color = Muted, style = MaterialTheme.typography.labelSmall, modifier = Modifier.padding(top = 8.dp))
            }
            if (state.connection != ConnectionStatus.CONNECTED) Text("Offline · reconnect to refresh allowances", color = Amber, style = MaterialTheme.typography.bodySmall)
            usage.error?.let { Text(it, color = MaterialTheme.colorScheme.error, style = MaterialTheme.typography.bodySmall, modifier = Modifier.padding(top = 8.dp)) }
        }
        if (!usage.loading && usage.providers.isEmpty()) item {
            Text(if (usage.error == null) "No provider allowance data available." else "Usage could not be loaded.", color = Muted)
        }
        items(usage.providers.sortedByDescending { provider -> state.sessions.any { it.providerName.isNotBlank() && canonicalUsageProvider(it.providerName) == canonicalUsageProvider(provider.provider) } }) { provider ->
            val sessions = state.sessions.filter { it.providerName.isNotBlank() && canonicalUsageProvider(it.providerName) == canonicalUsageProvider(provider.provider) }
            Surface(shape = RoundedCornerShape(16.dp), color = MaterialTheme.colorScheme.surface, modifier = Modifier.fillMaxWidth()) {
                Column(Modifier.padding(10.dp), verticalArrangement = Arrangement.spacedBy(6.dp)) {
                    Text(provider.name.ifBlank { provider.provider }, style = MaterialTheme.typography.titleLarge)
                    if (sessions.isNotEmpty()) {
                        Text("In use · ${sessions.size} session${if (sessions.size == 1) "" else "s"}", color = Teal, style = MaterialTheme.typography.labelMedium)
                        sessions.map { it.model }.filter { it.isNotBlank() }.distinct().forEach { Text(it, color = Muted, style = MaterialTheme.typography.bodySmall) }
                    }
                    if (provider.error != null) Text(provider.error, color = Amber, style = MaterialTheme.typography.bodySmall)
                    else if (!provider.available || provider.limits.isEmpty()) Text("Remaining allowance unavailable from this provider.", color = Muted, style = MaterialTheme.typography.bodySmall)
                    provider.limits.forEach { limit ->
                        Column(verticalArrangement = Arrangement.spacedBy(5.dp)) {
                            Text(limit.name, style = MaterialTheme.typography.titleSmall)
                            val remaining = limit.remainingPercent
                            Text(if (remaining == null) "Remaining allowance unavailable" else "${remaining.roundToInt()}% remaining", style = MaterialTheme.typography.bodyLarge, color = if (remaining != null && remaining <= 10) Amber else Ink)
                            if (remaining != null) LinearProgressIndicator(progress = { (remaining / 100).toFloat() }, modifier = Modifier.fillMaxWidth(), color = if (remaining <= 10) Amber else Teal, trackColor = Line)
                            limit.resetsAt?.let { Text("Resets ${usageTime(it)}", color = Muted, style = MaterialTheme.typography.bodySmall) }
                        }
                    }
                }
            }
        }
        item { Text("Each window or account has its own limit. Allowances are not added together. Provider data may be cached. Session tokens do not determine your remaining plan quota.", color = Muted, style = MaterialTheme.typography.bodySmall) }
        val unreported = state.sessions.filter { session -> usage.providers.none { canonicalUsageProvider(it.provider) == canonicalUsageProvider(session.providerName) } }
        if (unreported.isNotEmpty()) item {
            Text("Other models in use", style = MaterialTheme.typography.titleMedium)
            unreported.map { it.model.ifBlank { "Unknown model" } }.distinct().forEach { Text("$it · allowance unavailable", color = Muted, style = MaterialTheme.typography.bodySmall) }
        }
    }
}
