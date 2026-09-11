package dev.jcode.mobile.data

import org.junit.Assert.*
import org.junit.Test

class ThreadCatchUpTest {
    @Test fun readingOneMessageDoesNotReadUnseenHistoryAndSurvivesReload() {
        val session = catchUpSessionKey("server", "s")
        val old = catchUpMessageKey(TranscriptEntry("old", "assistant", "Unseen history"))
        val newest = catchUpMessageKey(TranscriptEntry("new", "assistant", "Actually viewed"))
        val ledger = CatchUpLedger()
        assertFalse(ledger.hasSeen(session, old))
        ledger.acknowledge(session, listOf(newest))
        val restored = CatchUpLedger(ledger.encode())
        assertTrue(restored.hasSeen(session, newest))
        assertFalse(restored.hasSeen(session, old))
        assertFalse(restored.hasSeen(catchUpSessionKey("other-server", "s"), newest))
    }
    @Test fun timestampedHistoryRetainsReadIdentityAcrossRollingWindowReindex() {
        val entry = TranscriptEntry("history-200", "assistant", "A stamped message", timestampUnixMs = 1789090000000L)
        assertEquals(catchUpMessageKey(entry), catchUpMessageKey(entry.copy(id = "history-0")))
        assertNotEquals(catchUpMessageKey(entry), catchUpMessageKey(entry.copy(timestampUnixMs = entry.timestampUnixMs!! + 1)))
        val snapshot = MobileSession("s", output = "Output")
        assertEquals(catchUpActivityKey(snapshot), catchUpActivityKey(snapshot.copy(lastActivityAgeSecs = 99)))
        assertNotEquals(catchUpActivityKey(snapshot), catchUpActivityKey(snapshot.copy(output = "New output")))
    }
    @Test fun changedLegacyContentAndToolOutputCannotInheritReadEvidence() {
        val original = TranscriptEntry("history:0", "assistant", "Result", tools = listOf(ToolEntry("tool", "bash", output = "pending")))
        assertNotEquals(catchUpMessageKey(original), catchUpMessageKey(original.copy(text = "Replacement history")))
        assertNotEquals(catchUpMessageKey(original), catchUpMessageKey(original.copy(reasoning = "New reasoning")))
        assertNotEquals(catchUpMessageKey(original), catchUpMessageKey(original.copy(tools = listOf(original.tools.single().copy(output = "done")))))
        assertFalse(catchUpEligible(original.copy(isStreaming = true)))
        assertFalse(catchUpEligible(TranscriptEntry("human", "user", "Sent by me")))
    }
    @Test fun ledgerRetentionIsBoundedAndEvictionNeverFalselyMarksRead() {
        val ledger = CatchUpLedger(maxSessions = 2, maxMessages = 2)
        val sessions = (0..2).map { catchUpSessionKey("host", "$it") }
        val messages = (0..2).map { catchUpMessageKey(TranscriptEntry("$it", "assistant", "$it")) }
        ledger.acknowledge(sessions[0], messages)
        assertFalse(ledger.hasSeen(sessions[0], messages[0]))
        assertTrue(ledger.hasSeen(sessions[0], messages[2]))
        ledger.acknowledge(sessions[1], listOf(messages[0]))
        ledger.acknowledge(sessions[2], listOf(messages[0]))
        assertFalse(ledger.hasSeen(sessions[0], messages[2]))
        assertEquals(2, ledger.encode().lines().size)
    }
    @Test fun loadedSearchUsesAttributionForDmAndBackgroundAndSearchesTools() {
        val entries = listOf(
            TranscriptEntry("human", "user", "needle"),
            TranscriptEntry("dm", "user", "[2026-09-10T20:00:00Z] DM from worker: needle"),
            TranscriptEntry("background", "user", "**Background task progress** needle"),
            TranscriptEntry("tool", "assistant", tools = listOf(ToolEntry("t", "bash", output = "NEEDLE")))
        )
        assertEquals(listOf("dm"), searchLoadedMessages(entries, "needle", "worker", "Main agent").map { it.id })
        assertEquals(listOf("background"), searchLoadedMessages(entries, "needle", "Background task progress", "Main agent").map { it.id })
        assertEquals(listOf("tool"), searchLoadedMessages(entries, "needle", "Main agent", "Main agent").map { it.id })
        assertEquals(4, searchLoadedMessages(entries, " needle ", null, "Main agent").size)
        assertTrue(searchLoadedMessages(entries, "not loaded", null, "Main agent").isEmpty())
    }
}
