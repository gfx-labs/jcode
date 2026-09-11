package dev.jcode.mobile.data

import org.junit.Assert.*
import org.junit.Test

class ThreadPresentationTest {
    private val session = MobileSession("s")
    private fun entry(id: String, text: String = id) = TranscriptEntry(id, "assistant", text)
    private fun delivery(id: Long, target: String = "s") = DeliveryResult(id, target, target, "message", DeliveryStatus.ACCEPTED)

    @Test fun newestIsIndexZeroWhileVisualOrderRemainsChronological() {
        val history = listOf(entry("oldest"), entry("middle"), entry("newest"))
        val rows = threadRowsNewestFirst(session, history, emptyList())
        assertEquals(listOf("message:newest", "message:middle", "message:oldest"), rows.map { it.key })
        assertEquals(history, rows.asReversed().map { (it as ThreadRow.Message).entry })
    }
    @Test fun liveOutputAndDeliveryReceiptsAreAtLatestEnd() {
        val rows = threadRowsNewestFirst(session.copy(isProcessing = true, output = "streaming"), listOf(entry("history")), listOf(delivery(1), delivery(2)))
        assertEquals(listOf("delivery:2:s", "delivery:1:s", "live:s", "message:history"), rows.map { it.key })
        assertTrue((rows[2] as ThreadRow.Message).live)
        assertTrue(rows.map { it.key }.distinct().size == rows.size)
    }
    @Test fun emptyThreadAndLateHistoryStayAnchoredAtIndexZero() {
        assertEquals(listOf(ThreadRow.Empty), threadRowsNewestFirst(session, emptyList(), emptyList()))
        val outputOnly = threadRowsNewestFirst(session.copy(output = "latest output"), emptyList(), emptyList())
        assertEquals("latest output", (outputOnly.first() as ThreadRow.Message).entry.text)
        val loaded = threadRowsNewestFirst(session, (0..100).map { entry("m$it") }, emptyList())
        assertEquals("message:m100", loaded.first().key)
    }
    @Test fun changingLiveTextKeepsStableRowIdentityAndTriggersContentChange() {
        val before = threadRowsNewestFirst(session.copy(isProcessing = true, output = "a"), listOf(entry("history")), emptyList())
        val after = threadRowsNewestFirst(session.copy(isProcessing = true, output = "a".repeat(10000)), listOf(entry("history")), emptyList())
        assertEquals(before.first().key, after.first().key)
        assertNotEquals(before, after)
    }
    @Test fun receiptsAreBoundedAndScopedToTheSelectedSession() {
        val rows = threadRowsNewestFirst(session, emptyList(), (0L..10L).map { delivery(it) } + delivery(99, "other"))
        val receipts = rows.filterIsInstance<ThreadRow.Delivery>()
        assertEquals((10L downTo 3L).toList(), receipts.map { it.result.requestId })
    }
    @Test fun scrollingUpPausesFollowAndReturningToBottomResumes() {
        assertTrue(followLatestAfterScroll(false, false, true, true))
        assertFalse(followLatestAfterScroll(false, true, true, true))
        assertFalse(followLatestAfterScroll(false, false, false, true))
        assertTrue(followLatestAfterScroll(false, false, true, false))
    }
    @Test fun programmaticScrollDoesNotDisableFollowing() {
        assertTrue(followLatestAfterScroll(true, true, false, true))
        assertFalse(followLatestAfterScroll(true, true, true, false))
    }
}
