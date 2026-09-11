package dev.jcode.mobile.data

import java.security.MessageDigest

/** Local device read evidence, never delivery receipts. Only viewport acknowledgements write it. */
class CatchUpLedger(encoded: String = "", private val maxSessions: Int = 64, private val maxMessages: Int = 2048) {
    private val seen = linkedMapOf<String, LinkedHashSet<String>>()
    init {
        encoded.lineSequence().takeLastBounded(maxSessions).forEach { line ->
            val parts = line.split(':', limit = 2)
            if (parts.size == 2 && parts[0].matches(Regex("[a-f0-9]{64}"))) {
                seen[parts[0]] = parts[1].split(',').filter { it.matches(Regex("[a-f0-9]{64}")) }.takeLast(maxMessages).toCollection(linkedSetOf())
            }
        }
    }
    fun hasSeen(sessionKey: String, messageKey: String): Boolean = messageKey in seen[sessionKey].orEmpty()
    fun acknowledge(sessionKey: String, messageKeys: Collection<String>): Boolean {
        if (messageKeys.isEmpty() || messageKeys.all { hasSeen(sessionKey, it) }) return false
        val entries = seen.remove(sessionKey) ?: linkedSetOf()
        messageKeys.forEach { entries.remove(it); entries.add(it) }
        while (entries.size > maxMessages) entries.remove(entries.first())
        seen[sessionKey] = entries
        while (seen.size > maxSessions) seen.remove(seen.keys.first())
        return true
    }
    fun encode(): String = seen.entries.joinToString("\n") { (key, value) -> "$key:${value.joinToString(",")}" }
}

private fun Sequence<String>.takeLastBounded(size: Int): List<String> {
    val result = ArrayDeque<String>()
    forEach { result.addLast(it); if (result.size > size) result.removeFirst() }
    return result.toList()
}
private fun catchUpHash(value: String): String {
    val digest = MessageDigest.getInstance("SHA-256").digest(value.toByteArray(Charsets.UTF_8))
    val hex = "0123456789abcdef"
    return buildString(64) { digest.forEach { append(hex[(it.toInt() and 255) ushr 4]); append(hex[it.toInt() and 15]) } }
}
fun catchUpSessionKey(host: String, sessionId: String): String = catchUpHash("$host\u0000$sessionId")
/** Include content for legacy positional IDs, preventing shifted/replaced history from appearing read. */
fun catchUpMessageKey(entry: TranscriptEntry): String = catchUpHash(buildString {
    val identity = entry.messageId ?: entry.timestampUnixMs?.let { "timestamp:$it:${entry.role}" } ?: entry.id
    append(identity).append('\u0000').append(entry.fingerprint())
    append('\u0000').append(entry.reasoning)
    entry.tools.forEach { tool ->
        listOf(tool.id, tool.name, tool.input, tool.output, tool.status, tool.error.orEmpty()).forEach {
            append('\u0000').append(it.length).append(':').append(it)
        }
    }
})
fun catchUpEligible(entry: TranscriptEntry): Boolean = !entry.isStreaming && !messageAttribution(entry).isHuman &&
    (entry.text.isNotBlank() || entry.reasoning.isNotBlank() || entry.tools.isNotEmpty())

fun searchSender(entry: TranscriptEntry, assistantName: String): String {
    val attribution = messageAttribution(entry)
    return attribution.sender ?: if (entry.role == "assistant") assistantName else attribution.label
}
fun searchLoadedMessages(entries: List<TranscriptEntry>, query: String, sender: String?, assistantName: String): List<TranscriptEntry> {
    val needle = query.trim()
    return entries.filter { entry ->
        (sender == null || searchSender(entry, assistantName) == sender) &&
            (needle.isEmpty() || entry.text.contains(needle, true) || entry.reasoning.contains(needle, true) ||
                entry.tools.any { listOf(it.name, it.input, it.output, it.error.orEmpty()).any { text -> text.contains(needle, true) } })
    }
}

/** Snapshot activity is a dot, never an unread message count. Initial discovery is only a baseline. */
fun catchUpActivityKey(session: MobileSession): String = catchUpHash(listOf(session.status, session.output,
    session.detail, session.currentTool, session.completionReport, session.isProcessing.toString()).joinToString("\u0000"))
