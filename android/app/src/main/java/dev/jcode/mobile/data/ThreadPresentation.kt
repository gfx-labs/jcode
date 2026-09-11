package dev.jcode.mobile.data

/** Index zero is the newest row. Compose reverseLayout places it at the bottom. */
sealed interface ThreadRow {
    val key: String
    data class Message(val entry: TranscriptEntry, val live: Boolean = false, val receipt: MessageReceipt? = null) : ThreadRow {
        override val key = if (live) "live:${entry.id}" else "message:${entry.id}"
    }
    data object Empty : ThreadRow { override val key = "empty" }
}

private val timestampEnvelope = Regex("^\\[\\d{4}-\\d{2}-\\d{2}T\\d{2}:\\d{2}:\\d{2}(?:\\.\\d+)?(?:Z|[+-]\\d{2}:\\d{2})]\\s*")
internal fun normalizedHumanText(text: String): String = text.replaceFirst(timestampEnvelope, "").trim()
internal fun TranscriptEntry.isHuman(): Boolean = role == "user" && messageAttribution(this).isHuman &&
    !normalizedHumanText(text).startsWith("[NOTIFICATION]") && !normalizedHumanText(text).startsWith("[System:") &&
    !normalizedHumanText(text).startsWith("[tool timing:")
internal fun TranscriptEntry.fingerprint(): String {
    val content = "$role\u0000${displayRole.orEmpty()}\u0000${if (isHuman()) normalizedHumanText(text) else text.trim()}"
    val digest = java.security.MessageDigest.getInstance("SHA-256").digest(content.toByteArray(Charsets.UTF_8))
    val hex = "0123456789abcdef"
    return buildString(64) { digest.forEach { append(hex[(it.toInt() and 255) ushr 4]); append(hex[it.toInt() and 15]) } }
}
internal fun historyBaseline(transcript: List<TranscriptEntry>): List<String> = transcript.filterNot { it.isStreaming }.takeLast(128).map { it.fingerprint() }

/** A surviving suffix locates insertion only. It must never prove delivery or read status. */
private fun insertionAnchor(baseline: List<String>?, current: List<String>): Int? {
    if (baseline == null) return null
    if (baseline.isEmpty()) return 0
    for (length in minOf(baseline.size, current.size) downTo 1) {
        val suffix = baseline.takeLast(length)
        val starts = (0..current.size - length).filter { current.subList(it, it + length) == suffix }
        if (starts.size == 1) return starts.single() + length
        if (starts.size > 1) return null
    }
    return null
}

/** Only a server-generated ID can reconcile a send. Identical text is never receipt evidence. */
internal fun reconcileDeliveries(sessionId: String, history: List<TranscriptEntry>, deliveries: List<DeliveryResult>): List<DeliveryResult> {
    val fingerprints = history.map { it.fingerprint() }
    val byMessageId = history.withIndex().filter { it.value.messageId != null }.groupBy { it.value.messageId }
    val claimedIds = deliveries.filter { it.sessionId == sessionId && it.messageId != null }.groupingBy { it.messageId }.eachCount()
    return deliveries.map { delivery ->
        if (delivery.sessionId != sessionId) return@map delivery
        val anchor = insertionAnchor(delivery.historyBaseline, fingerprints)
        val match = delivery.messageId?.takeIf { claimedIds[it] == 1 && delivery.status != DeliveryStatus.REJECTED }
            ?.let { byMessageId[it]?.singleOrNull() }
            ?.takeIf { it.value.role == "user" && (it.value.displayRole == null || it.value.displayRole == "user") && !it.value.isStreaming }?.index
        val response = match != null && history.drop(match + 1).any {
            it.role == "assistant" && !it.isStreaming && it.text.isNotBlank()
        }
        delivery.copy(matchedHistoryIndex = match, read = delivery.read || response,
            insertionIndex = (anchor ?: delivery.insertionIndex ?: history.size).coerceIn(0, history.size))
    }
}

fun threadRowsNewestFirst(session: MobileSession, transcript: List<TranscriptEntry>, deliveries: List<DeliveryResult>): List<ThreadRow> {
    val fingerprints = transcript.map { it.fingerprint() }
    val outgoing = reconcileDeliveries(session.id, transcript, deliveries).filter { it.sessionId == session.id }
    val merged = outgoing.filter { it.matchedHistoryIndex != null }.associateBy { it.matchedHistoryIndex!! }
    val pending = mutableMapOf<Int, MutableList<DeliveryResult>>()
    var previousPosition = 0
    outgoing.forEach { delivery ->
        val match = delivery.matchedHistoryIndex
        if (match != null) previousPosition = maxOf(previousPosition, match + 1)
        else {
            val position = maxOf(previousPosition, insertionAnchor(delivery.historyBaseline, fingerprints)
                ?: delivery.insertionIndex ?: transcript.size).coerceAtMost(transcript.size)
            pending.getOrPut(position) { mutableListOf() }.add(delivery)
            previousPosition = position
        }
    }
    val chronological = buildList<ThreadRow> {
        for (index in 0..transcript.size) {
            pending[index].orEmpty().forEach { add(ThreadRow.Message(TranscriptEntry(it.localId, "user", it.text), receipt = it.receipt)) }
            if (index < transcript.size) {
                val delivery = merged[index]
                add(ThreadRow.Message(if (delivery == null) transcript[index] else transcript[index].copy(id = delivery.localId, text = delivery.text), receipt = delivery?.receipt))
            }
        }
        val lastAssistant = transcript.lastOrNull { it.role == "assistant" }?.text?.trim()
        val unchangedSinceSend = outgoing.lastOrNull()?.let { it.matchedHistoryIndex == null && it.outputAtSend == session.output } == true
        if (session.output.isNotBlank() && lastAssistant?.endsWith(session.output.trim()) != true && !unchangedSinceSend && (session.isProcessing || transcript.isEmpty())) {
            add(ThreadRow.Message(TranscriptEntry(session.id, "assistant", session.output, isStreaming = session.isProcessing), live = true))
        }
        if (isEmpty()) add(ThreadRow.Empty)
    }
    return chronological.asReversed()
}

/** Only user scrolling changes follow mode. Content/layout changes must not. */
fun followLatestAfterScroll(autoScrolling: Boolean, scrolling: Boolean, atLatest: Boolean, wasFollowing: Boolean): Boolean =
    if (autoScrolling) wasFollowing else !scrolling && atLatest
