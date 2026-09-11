package dev.jcode.mobile.ui

import androidx.compose.foundation.clickable
import androidx.compose.foundation.horizontalScroll
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import dev.jcode.mobile.data.*

@Composable
internal fun ThreadSearchDialog(entries: List<TranscriptEntry>, assistantName: String, onDismiss: () -> Unit, onJump: (String) -> Unit) {
    var query by rememberSaveable { mutableStateOf("") }
    var sender by rememberSaveable { mutableStateOf<String?>(null) }
    val senders = remember(entries, assistantName) { entries.map { searchSender(it, assistantName) }.distinct().sorted() }
    val results = remember(entries, query, sender, assistantName) { searchLoadedMessages(entries, query, sender, assistantName).asReversed() }
    AlertDialog(onDismissRequest = onDismiss, modifier = Modifier.testTag("thread-search-dialog"),
        title = { Text("Search this session") }, text = {
            Column(verticalArrangement = Arrangement.spacedBy(8.dp)) {
                Text("Loaded history only · ${entries.size} messages. Older server history is not searched.", style = MaterialTheme.typography.bodySmall)
                OutlinedTextField(query, { query = it.take(500) }, label = { Text("Find text, reasoning or tool output") },
                    singleLine = true, modifier = Modifier.fillMaxWidth().testTag("thread-search-input"))
                Text("Sender", style = MaterialTheme.typography.labelMedium)
                Row(Modifier.fillMaxWidth().horizontalScroll(rememberScrollState()).testTag("thread-sender-filter"),
                    horizontalArrangement = Arrangement.spacedBy(6.dp)) {
                    FilterChip(selected = sender == null, onClick = { sender = null }, label = { Text("Everyone") },
                        modifier = Modifier.testTag("thread-sender-everyone"))
                    senders.forEach { name ->
                        FilterChip(selected = sender == name, onClick = { sender = name }, label = { Text(name) },
                            modifier = Modifier.testTag("thread-sender-option-$name"))
                    }
                }
                Text("${results.size} matches", style = MaterialTheme.typography.labelMedium, modifier = Modifier.testTag("thread-search-count"))
                LazyColumn(Modifier.fillMaxWidth().heightIn(max = 300.dp).testTag("thread-search-results"), verticalArrangement = Arrangement.spacedBy(8.dp)) {
                    if (results.isEmpty()) item { Text("No matches in loaded history.") }
                    items(results, key = { it.id }) { entry ->
                        Surface(Modifier.fillMaxWidth().clickable { onJump(entry.id) }.testTag("search-result-${entry.id}"), tonalElevation = 2.dp, shape = MaterialTheme.shapes.small) {
                            Column(Modifier.padding(10.dp)) {
                                Text(searchSender(entry, assistantName), style = MaterialTheme.typography.labelLarge)
                                Text(entry.text.ifBlank { entry.reasoning.ifBlank { "Tool output" } }, maxLines = 3, overflow = TextOverflow.Ellipsis, style = MaterialTheme.typography.bodySmall)
                                MessageTimestamp(entry.timestampUnixMs)
                                Text("Jump to message", style = MaterialTheme.typography.labelSmall)
                            }
                        }
                    }
                }
            }
        }, confirmButton = { TextButton(onClick = onDismiss) { Text("Close") } })
}
