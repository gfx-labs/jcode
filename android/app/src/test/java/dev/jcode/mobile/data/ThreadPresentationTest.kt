package dev.jcode.mobile.data

import org.junit.Assert.*
import org.junit.Test

class ThreadPresentationTest {
    private val session = MobileSession("s")
    private fun entry(id: String, text: String = id) = TranscriptEntry(id, "assistant", text)
    private fun sent(id: Long, history: List<TranscriptEntry>, text: String = "message") = DeliveryResult(id, "s", "s", text, DeliveryStatus.ACCEPTED,
        historyBaseline = historyBaseline(history), insertionIndex = history.size, messageId = "mobile:$id")
    private fun user(id: Long, text: String = "message") = TranscriptEntry("history-$id", "user", text, messageId = "mobile:$id")
    private fun rows(history: List<TranscriptEntry>, vararg sends: DeliveryResult) = threadRowsNewestFirst(session, history, sends.toList()).asReversed().filterIsInstance<ThreadRow.Message>()

    @Test fun newestIsIndexZeroWhileVisualOrderRemainsChronological() {
        val history = listOf(entry("old"), entry("new"))
        assertEquals(listOf("message:new", "message:old"), threadRowsNewestFirst(session, history, emptyList()).map { it.key })
    }
    @Test fun localSendStaysBetweenItsHistoryBoundaryAndNewReplies() {
        val old = listOf(entry("old")); val send = sent(1, old)
        val messages = rows(old + entry("new"), send)
        assertEquals(listOf("old", "message", "new"), messages.map { it.entry.text })
        assertEquals(MessageReceipt.SENT, messages[1].receipt)
    }
    @Test fun onlyExactIdMergesAndResponseProvesRead() {
        val old = listOf(entry("old")); val send = sent(1, old)
        val before = rows(old, send).last()
        val delivered = rows(old + user(1, "[2026-09-11T02:00:00Z] message"), send)
        assertEquals(2, delivered.size); assertEquals(before.key, delivered.last().key)
        assertEquals("message", delivered.last().entry.text)
        assertEquals(MessageReceipt.SENT, delivered.last().receipt)
        val read = rows(old + user(1) + entry("reply"), send)
        assertEquals(MessageReceipt.READ, read[1].receipt)
        assertEquals("reply", read.last().entry.text)
    }
    @Test fun identicalExternalTextCannotProveReadOrHideUnknownSend() {
        val old = listOf(entry("old")); val unknown = sent(1, old).copy(status = DeliveryStatus.UNKNOWN, messageId = null)
        val messages = rows(old + TranscriptEntry("external", "user", "message") + entry("reply"), unknown)
        assertEquals(2, messages.count { it.entry.text == "message" })
        assertEquals(MessageReceipt.UNKNOWN, messages.single { it.receipt != null }.receipt)
        assertFalse(rows(old + user(2) + entry("reply"), sent(1, old)).any { it.receipt == MessageReceipt.READ })
    }
    @Test fun identicalSendsKeepDistinctIdsAndOrderDuringPartialHistory() {
        val old = listOf(entry("old")); val a = sent(1, old); val b = sent(2, old)
        val partial = rows(old + user(1), a, b)
        assertEquals(listOf("old", "message", "message"), partial.map { it.entry.text })
        assertEquals(listOf("message:local:s:1", "message:local:s:2"), partial.drop(1).map { it.key })
        val complete = rows(old + user(1) + user(2) + entry("reply"), a, b)
        assertEquals(2, complete.count { it.receipt == MessageReceipt.READ })
    }
    @Test fun duplicateServerIdsCannotCreateReadEvidence() {
        val old = listOf(entry("old")); val messages = rows(old + user(1) + user(1) + entry("reply"), sent(1, old))
        assertFalse(messages.any { it.receipt == MessageReceipt.READ })
    }
    @Test fun exactIdSurvivesHistoryWindowChangesAndDoesNotMatchAnotherSession() {
        val old = listOf(entry("old")); val send = sent(1, old)
        assertEquals(MessageReceipt.READ, rows(listOf(user(1), entry("reply")), send).first().receipt)
        assertTrue(rows(old, send.copy(sessionId = "other")).all { it.receipt == null })
    }
    @Test fun cachedOutputDoesNotPretendToBeNewReplyAndNewOutputFollowsSend() {
        val old = listOf(entry("old", "old response")); val send = sent(1, old).copy(outputAtSend = "old tail")
        assertFalse(threadRowsNewestFirst(session.copy(isProcessing = true, output = "old tail"), old, listOf(send)).filterIsInstance<ThreadRow.Message>().any { it.live })
        val changed = threadRowsNewestFirst(session.copy(isProcessing = true, output = "new response"), old, listOf(send))
        assertEquals("new response", (changed.first() as ThreadRow.Message).entry.text)
        assertTrue((changed.first() as ThreadRow.Message).live)
    }
    @Test fun emptyThreadAndProgrammaticFollowingStayStable() {
        assertEquals(listOf(ThreadRow.Empty), threadRowsNewestFirst(session, emptyList(), emptyList()))
        assertTrue(followLatestAfterScroll(true, true, false, true))
        assertFalse(followLatestAfterScroll(false, true, true, true))
        assertFalse(followLatestAfterScroll(false, false, false, true))
        assertTrue(followLatestAfterScroll(false, false, true, false))
    }
}
