package dev.jcode.mobile.data

import org.json.JSONObject
import org.junit.Assert.*
import org.junit.Test

class MessageTimestampTest {
    private fun history(timestamp: String = ""): List<TranscriptEntry> = WireCodec.history(JSONObject("""
        {"messages":[{"role":"user","content":"hello","message_id":"mobile:1"$timestamp}]}
    """))

    @Test fun historyUsesOnlyIntegralStoredTimestamp() {
        assertEquals(1789096017543L, history(",\"timestamp_unix_ms\":1789096017543").single().timestampUnixMs)
        assertEquals(0L, history(",\"timestamp_unix_ms\":0").single().timestampUnixMs)
        assertEquals(-1L, history(",\"timestamp_unix_ms\":-1").single().timestampUnixMs)
        for (value in listOf("null", "\"1789096017543\"", "1.5", "true", "{}", "9223372036854775808", "9223372036854775807", "-62135596800001", "253402300800000")) {
            assertNull(history(",\"timestamp_unix_ms\":$value").single().timestampUnixMs)
        }
        assertNull(history().single().timestampUnixMs)
        assertEquals(history(), history()) // Repeated polling does not date legacy history.
    }

    @Test fun localTimestampSurvivesReceiptUpdatesAndServerReconciliation() {
        val session = MobileSession("s", "Jcode")
        val delivery = DeliveryResult(1, "s", "Jcode", "hello", DeliveryStatus.SENDING, sentAtUnixMs = 123L)
        fun message(entries: List<TranscriptEntry>, send: DeliveryResult) =
            (threadRowsNewestFirst(session, entries, listOf(send)).single() as ThreadRow.Message)
        assertEquals(123L, message(emptyList(), delivery).entry.timestampUnixMs)
        val acknowledged = delivery.copy(status = DeliveryStatus.ACCEPTED, messageId = "mobile:1")
        assertEquals(123L, message(history(), acknowledged).entry.timestampUnixMs)
        val persisted = message(history(",\"timestamp_unix_ms\":456"), acknowledged)
        assertEquals(456L, persisted.entry.timestampUnixMs)
        assertEquals(MessageReceipt.SENT, persisted.receipt)
        assertEquals(delivery.localId, persisted.entry.id)
    }

    @Test fun liveTailNeverUsesCurrentTime() {
        val rows = threadRowsNewestFirst(MobileSession("s", "Jcode", output = "working", isProcessing = true), emptyList(), emptyList())
        assertNull((rows.single() as ThreadRow.Message).entry.timestampUnixMs)
    }
}
