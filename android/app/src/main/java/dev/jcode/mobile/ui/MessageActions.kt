package dev.jcode.mobile.ui

import androidx.compose.foundation.layout.*
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.outlined.*
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalClipboardManager
import androidx.compose.ui.text.AnnotatedString
import androidx.compose.ui.unit.dp

/** Reply is text in the existing draft, never an implicit network action. */
internal fun quotedReply(sender: String, text: String): String = buildString {
    append("> ").append(sender.replace('\n', ' ').replace('\r', ' ')).append(":\n")
    val excerpt = text.take(2000)
    excerpt.lineSequence().forEach { append("> ").append(it).append('\n') }
    if (text.length > excerpt.length) append("> …\n")
    append('\n')
}

@Composable
internal fun MessageActions(sender: String, text: String, onQuote: ((String) -> Unit)?) {
    var open by remember { mutableStateOf(false) }
    val clipboard = LocalClipboardManager.current
    Box {
        IconButton(onClick = { open = true }, modifier = Modifier.size(32.dp)) {
            Icon(Icons.Outlined.MoreHoriz, "Message actions from $sender", Modifier.size(18.dp), tint = Muted)
        }
        DropdownMenu(open, onDismissRequest = { open = false }) {
            DropdownMenuItem(text = { Text("Copy message") }, leadingIcon = { Icon(Icons.Outlined.ContentCopy, null) },
                onClick = { clipboard.setText(AnnotatedString(text)); open = false })
            if (onQuote != null) DropdownMenuItem(text = { Text("Quote reply") }, leadingIcon = { Icon(Icons.Outlined.FormatQuote, null) },
                onClick = { onQuote(quotedReply(sender, text)); open = false })
        }
    }
}
