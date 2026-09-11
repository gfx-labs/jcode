package dev.jcode.mobile.ui

import androidx.compose.foundation.background
import androidx.compose.foundation.horizontalScroll
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.text.selection.SelectionContainer
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.outlined.ContentCopy
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.Modifier
import androidx.compose.ui.Alignment
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.platform.LocalClipboardManager
import androidx.compose.ui.text.*
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.font.FontStyle
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextDecoration
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import org.commonmark.node.*
import org.commonmark.node.Paragraph as MdParagraph
import org.commonmark.node.Text as MdText
import androidx.compose.ui.draw.drawBehind
import androidx.compose.ui.geometry.Size
import org.commonmark.node.Image as MdImage
import org.commonmark.parser.Parser
import org.commonmark.ext.gfm.tables.*
import org.commonmark.ext.gfm.strikethrough.*
import org.commonmark.ext.task.list.items.*
import org.commonmark.ext.autolink.AutolinkExtension
import java.net.URI

private val markdownParser = Parser.builder().extensions(listOf(
    TablesExtension.create(), StrikethroughExtension.create(), TaskListItemsExtension.create(), AutolinkExtension.create()
)).build()

private fun Node.children(): List<Node> = buildList {
    var child = firstChild
    while (child != null) { add(child); child = child.next }
}

/** Never execute HTML, local paths, intent URIs or remote image fetches from a message. */
internal fun safeMarkdownLink(destination: String): String? = runCatching {
    val uri = URI(destination)
    when (uri.scheme?.lowercase()) {
        "https", "http" -> destination.takeIf { !uri.host.isNullOrBlank() && uri.userInfo == null }
        "mailto" -> destination.takeIf { uri.schemeSpecificPart.isNotBlank() }
        else -> null
    }
}.getOrNull()

private val structuralMarkdownParser = Parser.builder().build()

/** Extension visitors recurse, so screen the core parser's tree before running them. */
internal fun parseSafeMarkdown(source: String): Node {
    val structural = structuralMarkdownParser.parse(source)
    var node: Node? = structural
    var depth = 0
    var visited = 0
    while (node != null) {
        if (depth > 48 || visited++ > 10000) return structural
        val current: Node = node
        if (current.firstChild != null) { node = current.firstChild; depth++ }
        else {
            var cursor = current
            while (cursor !== structural && cursor.next == null) {
                cursor = cursor.parent ?: break
                depth--
            }
            node = if (cursor === structural) null else cursor.next
        }
    }
    return markdownParser.parse(source)
}

/** Iterative and bounded: never delegate deep remote content to a recursive renderer. */
internal fun boundedMarkdownText(root: Node): String = buildString {
    var node: Node? = root
    var visited = 0
    while (node != null && visited++ < 10000 && length < 16000) {
        val current: Node = node ?: break
        val literal = when (current) {
            is MdText -> current.literal
            is Code -> current.literal
            is FencedCodeBlock -> current.literal
            is IndentedCodeBlock -> current.literal
            is HtmlInline -> current.literal
            is HtmlBlock -> current.literal
            is SoftLineBreak, is HardLineBreak -> "\n"
            else -> ""
        }
        append(literal.take(16000 - length))
        if (current.firstChild != null) node = current.firstChild
        else {
            var cursor = current
            while (cursor !== root && cursor.next == null) cursor = cursor.parent ?: break
            node = if (cursor === root) null else cursor.next
        }
    }
    if (node != null) append("\n[Additional deeply nested content omitted]")
}

@Composable
internal fun MarkdownMessage(source: String, modifier: Modifier = Modifier, style: TextStyle = MaterialTheme.typography.bodyLarge) {
    val resolvedStyle = if (style.color == Color.Unspecified) style.copy(color = Ink) else style
    val document = remember(source) { parseSafeMarkdown(source) }
    // Preserve plain transcript text semantics and line breaks, including tall live output.
    val plain = remember(source) {
        document.children().all { it is MdParagraph && it.children().all { n -> n is MdText || n is SoftLineBreak } } &&
            !source.contains('\\') && !source.contains('&')
    }
    SelectionContainer(modifier) {
        if (plain) Text(source, style = resolvedStyle)
        else Column(Modifier.fillMaxWidth(), verticalArrangement = Arrangement.spacedBy(10.dp)) {
            document.children().forEach { MarkdownBlock(it, resolvedStyle, 0) }
        }
    }
}

@Composable
private fun MarkdownBlock(node: Node, style: TextStyle, depth: Int) {
    if (depth > 24) { Text(boundedMarkdownText(node), style = style); return }
    when (node) {
        is MdParagraph -> Text(markdownInline(node), style = style)
        is Heading -> Text(markdownInline(node), style = style.copy(
            fontSize = when (node.level) { 1 -> 25.sp; 2 -> 22.sp; 3 -> 19.sp; else -> 17.sp },
            lineHeight = when (node.level) { 1 -> 32.sp; 2 -> 29.sp; else -> 25.sp }, fontWeight = FontWeight.Bold))
        is FencedCodeBlock -> MarkdownCode(node.literal, node.info.substringBefore(' '))
        is IndentedCodeBlock -> MarkdownCode(node.literal, "code")
        is BlockQuote -> {
            val stripe = Cobalt.copy(alpha = .6f)
            Column(Modifier.fillMaxWidth().drawBehind { drawRect(stripe, size = Size(3.dp.toPx(), size.height)) }.padding(start = 12.dp), verticalArrangement = Arrangement.spacedBy(6.dp)) {
                node.children().forEach { MarkdownBlock(it, style.copy(color = Muted), depth + 1) }
            }
        }
        is BulletList, is OrderedList -> Column(verticalArrangement = Arrangement.spacedBy(5.dp)) {
            val start = if (node is OrderedList) node.startNumber else 1
            node.children().forEachIndexed { index, item ->
                val marker = (item.children() + item.children().flatMap { it.children() }).filterIsInstance<TaskListItemMarker>().firstOrNull()
                Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                    Text(if (marker != null) (if (marker.isChecked) "☑" else "☐") else if (node is OrderedList) "${start + index}." else "•",
                        style = style, color = if (marker?.isChecked == true) Teal else Ink, modifier = Modifier.widthIn(min = 16.dp))
                    Column(Modifier.weight(1f), verticalArrangement = Arrangement.spacedBy(5.dp)) {
                        item.children().forEach { MarkdownBlock(it, style, depth + 1) }
                    }
                }
            }
        }
        is TableBlock -> MarkdownTable(node, style)
        is ThematicBreak -> HorizontalDivider(color = Line, modifier = Modifier.padding(vertical = 4.dp))
        is HtmlBlock -> Text(node.literal.trimEnd(), style = style.copy(fontFamily = FontFamily.Monospace))
        else -> node.children().forEach { MarkdownBlock(it, style, depth + 1) }
    }
}

@Composable
private fun markdownInline(node: Node): AnnotatedString {
    val linkColor = Cobalt
    val codeBackground = MaterialTheme.colorScheme.surfaceVariant
    return remember(node, linkColor, codeBackground) {
        buildAnnotatedString {
            fun visit(current: Node, depth: Int = 0) {
                if (depth > 48) { append(boundedMarkdownText(current)); return }
                fun children() { current.children().forEach { visit(it, depth + 1) } }
                when (current) {
                    is MdText -> append(current.literal)
                    is Code -> withStyle(SpanStyle(fontFamily = FontFamily.Monospace, background = codeBackground)) { append(current.literal) }
                    is StrongEmphasis -> withStyle(SpanStyle(fontWeight = FontWeight.Bold)) { children() }
                    is Emphasis -> withStyle(SpanStyle(fontStyle = FontStyle.Italic)) { children() }
                    is Strikethrough -> withStyle(SpanStyle(textDecoration = TextDecoration.LineThrough)) { children() }
                    is Link -> {
                        val safe = safeMarkdownLink(current.destination)
                        if (safe != null) withLink(LinkAnnotation.Url(safe, TextLinkStyles(style = SpanStyle(color = linkColor, textDecoration = TextDecoration.Underline)))) { children() }
                        else children()
                    }
                    is MdImage -> {
                        append("Image: ")
                        val safe = safeMarkdownLink(current.destination)
                        if (safe != null) withLink(LinkAnnotation.Url(safe, TextLinkStyles(style = SpanStyle(color = linkColor, textDecoration = TextDecoration.Underline)))) {
                            if (current.firstChild == null) append("open image") else children()
                        } else children()
                    }
                    is HardLineBreak -> append('\n')
                    is SoftLineBreak -> append('\n')
                    is HtmlInline -> append(current.literal)
                    is TaskListItemMarker -> Unit
                    else -> children()
                }
            }
            visit(node)
        }
    }
}

@Composable
internal fun MarkdownCode(code: String, language: String = "code") {
    val clipboard = LocalClipboardManager.current
    var copied by remember(code) { mutableStateOf(false) }
    Surface(color = MaterialTheme.colorScheme.surfaceVariant, shape = RoundedCornerShape(10.dp), modifier = Modifier.fillMaxWidth()) {
        Column {
            Row(Modifier.fillMaxWidth().padding(start = 12.dp), verticalAlignment = Alignment.CenterVertically) {
                Text(language.ifBlank { "code" }.take(40), Modifier.weight(1f), style = MaterialTheme.typography.labelSmall, color = Muted)
                TextButton(onClick = { clipboard.setText(AnnotatedString(code)); copied = true }) {
                    Icon(Icons.Outlined.ContentCopy, "Copy code", Modifier.size(16.dp))
                    Spacer(Modifier.width(5.dp)); Text(if (copied) "Copied" else "Copy")
                }
            }
            Text(code.trimEnd('\n'), fontFamily = FontFamily.Monospace, style = MaterialTheme.typography.bodyMedium,
                softWrap = false, modifier = Modifier.fillMaxWidth().horizontalScroll(rememberScrollState()).padding(start = 12.dp, end = 12.dp, bottom = 12.dp))
        }
    }
}

@Composable
private fun MarkdownTable(table: TableBlock, style: TextStyle) {
    val rows = table.children().flatMap { it.children() }.filterIsInstance<TableRow>()
    val columns = rows.maxOfOrNull { it.children().size } ?: return
    BoxWithConstraints(Modifier.fillMaxWidth()) {
        val cellWidth = (maxWidth / columns.coerceAtLeast(1)).coerceIn(110.dp, 240.dp)
        Column(Modifier.horizontalScroll(rememberScrollState())) {
            rows.forEach { row ->
                val cells = row.children().filterIsInstance<TableCell>()
                Row {
                    cells.forEach { cell ->
                        val alignment = when (cell.alignment) { TableCell.Alignment.CENTER -> TextAlign.Center; TableCell.Alignment.RIGHT -> TextAlign.End; else -> TextAlign.Start }
                        Box(Modifier.width(cellWidth).background(if (cell.isHeader) MaterialTheme.colorScheme.surfaceVariant else Color.Transparent).padding(9.dp)) {
                            Text(markdownInline(cell), style = style.copy(fontSize = 14.sp, lineHeight = 21.sp,
                                fontWeight = if (cell.isHeader) FontWeight.SemiBold else FontWeight.Normal, textAlign = alignment), modifier = Modifier.fillMaxWidth())
                        }
                    }
                }
                HorizontalDivider(color = Line, modifier = Modifier.width(cellWidth * columns))
            }
        }
    }
}
