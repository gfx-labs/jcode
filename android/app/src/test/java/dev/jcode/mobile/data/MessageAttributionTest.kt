package dev.jcode.mobile.data

import org.json.JSONObject
import org.junit.Assert.*
import org.junit.Test

class MessageAttributionTest {
    @Test fun legacyNotificationsAreNotHumanAndRetainTheirContent() {
        val examples = listOf(
            "DM from falcon: hello now" to "DM · falcon",
            "broadcast from fox: finished" to "Broadcast · fox",
            "#review from fox: take a look" to "#review · fox",
            "**Background task** `abc` · ✓ completed\n\noutput" to "Background task",
            "**Background task stalled** `abc` · no output" to "Background task stalled",
            "**Background task started** `abc`" to "Background task started",
            "**Background task progress** `abc`" to "Background task progress",
            "[2026-09-11T01:50:17.586Z] DM from fox: hello" to "DM · fox",
            "[auto] Continue the work below." to "Automation",
            "<system-reminder>Context</system-reminder>" to "System",
            "[Scheduled task]\nCheck progress" to "Scheduled task"
        )
        val messages = org.json.JSONArray()
        examples.forEach { (text, _) -> messages.put(JSONObject().put("role", "user").put("content", text)) }
        val decoded = WireCodec.history(JSONObject().put("messages", messages))
        examples.zip(decoded).forEach { (example, entry) ->
            assertEquals(example.first, entry.text)
            assertEquals(example.second, messageAttribution(entry).label)
            assertFalse(messageAttribution(entry).isHuman)
        }
    }
    @Test fun explicitRolesAndHumanTextAreNotConfusedWithNotificationBodies() {
        assertEquals("System", messageAttribution(TranscriptEntry("s", "user", "Injected notice", displayRole = "system")).label)
        assertEquals("Background task", messageAttribution(TranscriptEntry("b", "background_task", "finished")).label)
        assertEquals("Jcode", messageAttribution(TranscriptEntry("a", "assistant", "DM from fox: quoted example")).label)
        assertEquals("Tool output", messageAttribution(TranscriptEntry("t", "tool", "DM from fox: log line")).label)
        listOf("Fix this", "Please inspect DM from fox: hello", "> DM from fox: quoted", "Example:\n**Background task** `abc`", "```\nDM from fox: example\n```").forEach {
            assertTrue(messageAttribution(TranscriptEntry("u", "user", it)).isHuman)
        }
        assertFalse(messageAttribution(TranscriptEntry("x", "future_notice", "notice")).isHuman)
    }
}
