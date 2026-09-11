package dev.jcode.mobile.data

/** Index zero is the newest row. Compose reverseLayout places it at the bottom. */
sealed interface ThreadRow {
    val key: String
    data class Message(val entry: TranscriptEntry, val live: Boolean = false) : ThreadRow {
        override val key = if (live) "live:${entry.id}" else "message:${entry.id}"
    }
    data class Delivery(val result: DeliveryResult) : ThreadRow {
        override val key = "delivery:${result.requestId}:${result.sessionId}"
    }
    data object Empty : ThreadRow { override val key = "empty" }
}

fun threadRowsNewestFirst(session: MobileSession, transcript: List<TranscriptEntry>, deliveries: List<DeliveryResult>): List<ThreadRow> = buildList {
    addAll(deliveries.filter { it.sessionId == session.id }.takeLast(8).asReversed().map { ThreadRow.Delivery(it) })
    if (transcript.isNotEmpty() && session.isProcessing && session.output.isNotBlank()) {
        add(ThreadRow.Message(TranscriptEntry(session.id, "assistant", session.output, isStreaming = true), live = true))
    }
    addAll(transcript.asReversed().map { ThreadRow.Message(it) })
    if (transcript.isEmpty()) {
        if (session.output.isNotBlank()) add(ThreadRow.Message(TranscriptEntry("latest:${session.id}", "assistant", session.output)))
        else add(ThreadRow.Empty)
    }
}

/** Only user scrolling changes follow mode. Content/layout changes must not. */
fun followLatestAfterScroll(autoScrolling: Boolean, scrolling: Boolean, atLatest: Boolean, wasFollowing: Boolean): Boolean =
    if (autoScrolling) wasFollowing else !scrolling && atLatest
