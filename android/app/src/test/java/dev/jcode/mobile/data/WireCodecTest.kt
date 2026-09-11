package dev.jcode.mobile.data

import org.json.JSONObject
import org.junit.Assert.*
import org.junit.Test

class WireCodecTest {
    @Test fun newlineFramesAndUnknownEvents() {
        val events = WireCodec.decodeFrame("\n{\"type\":\"ack\",\"id\":2}\r\n{\"type\":\"future_event\",\"x\":true}\n")
        assertEquals(2, events.size)
        val initial = MobileState()
        assertEquals(initial, MobileReducer.reduce(initial, events.last()))
    }
    @Test(expected = IllegalArgumentException::class) fun rejectsMissingType() { WireCodec.decodeFrame("{\"id\":1}") }
    @Test fun exactLightweightRequests() {
        val encoded = WireCodec.request("mobile_message", 42, "session_id" to "abc", "content" to "quote \" and\nline")
        assertTrue(encoded.endsWith("\n"))
        val json = JSONObject(encoded)
        assertEquals("mobile_message", json.getString("type"))
        assertEquals("quote \" and\nline", json.getString("content"))
        assertFalse(json.has("allow_session_takeover"))
    }
    @Test fun realSwarmOptionalAndTupleFields() {
        // Shape copied from SwarmMemberStatus serde contract in crates/jcode-protocol/src/lib.rs.
        val session = WireCodec.session(JSONObject("""{"session_id":"s1","friendly_name":"fox","status":"running","detail":null,"report_back_to_session_id":"parent","todo_progress":[2,3],"todo_items":[{"content":"Verify gateway","status":"in_progress","tool_intents":[]}],"runtime":{"model":"gpt-5"},"output_tail":"hello"}"""))
        assertEquals("parent", session.parentId)
        assertEquals("", session.detail)
        assertEquals(2, session.todosCompleted)
        assertEquals(3, session.todosTotal)
        assertEquals("gpt-5", session.model)
        assertEquals("Verify gateway", session.todos.single().content)
    }
    @Test fun realHistoryToolRecordAndNulls() {
        val event = JSONObject("""{"type":"comm_context_history","id":7,"session_id":"s","messages":[{"role":"user","content":"Inspect"},{"role":"assistant","content":"Checking","tool_calls":["t"]},{"role":"tool","content":"ok","tool_data":{"id":"t","name":"bash","input":"{\"command\":\"ls\"}","output":"ok","error":null}}]}""")
        val state = MobileReducer.reduce(MobileState(), event)
        val messages = state.transcripts.getValue("s")
        assertEquals(2, messages.size)
        assertEquals("bash", messages.last().tools.single().name)
        assertNull(messages.last().tools.single().error)
    }
    @Test fun retryRollbackDiscardsPartialNotHistory() {
        var state = MobileState(transcripts = mapOf("s" to listOf(TranscriptEntry("old", "assistant", "completed"))))
        state = MobileReducer.reduce(state, JSONObject("""{"type":"text_delta","text":"partial"}"""), "s")
        state = MobileReducer.reduce(state, JSONObject("""{"type":"reasoning_delta","text":"reason"}"""), "s")
        state = MobileReducer.reduce(state, JSONObject("""{"type":"retry_rollback","attempt":1,"max":3}"""), "s")
        assertEquals(listOf("completed"), state.transcripts.getValue("s").map { it.text })
    }
    @Test fun textReplaceAndToolLifecycle() {
        var state = MobileState()
        listOf("""{"type":"text_delta","text":"old"}""", """{"type":"text_replace","text":"new"}""",
            """{"type":"tool_start","id":"t","name":"read"}""", """{"type":"tool_input","delta":"input"}""",
            """{"type":"tool_done","id":"t","name":"read","output":"out","error":"failed"}""", """{"type":"message_end","stop_reason":null}""")
            .forEach { state = MobileReducer.reduce(state, JSONObject(it), "s") }
        val message = state.transcripts.getValue("s").single()
        assertEquals("new", message.text)
        assertEquals("input", message.tools.single().input)
        assertEquals("failed", message.tools.single().status)
        assertFalse(message.isStreaming)
    }
    @Test fun deliveryResultsArePerRecipientAndDisconnectNeverResends() {
        var state = MobileState(deliveries = listOf(
            DeliveryResult(1, "a", "A", "hello", DeliveryStatus.SENDING), DeliveryResult(2, "b", "B", "hello", DeliveryStatus.SENDING)))
        state = MobileReducer.reduce(state, JSONObject("""{"type":"mobile_delivery","id":1,"session_id":"a","status":"accepted","message":"Queued"}"""))
        state = MobileReducer.disconnected(state)
        assertEquals(DeliveryStatus.ACCEPTED, state.deliveries[0].status)
        assertEquals(DeliveryStatus.UNKNOWN, state.deliveries[1].status)
        assertTrue(state.deliveries[1].detail.contains("Not resent"))
    }
    @Test fun snapshotReplacesRemovedSessions() {
        val old = MobileState(sessions = listOf(MobileSession("gone")))
        val fresh = MobileReducer.reduce(old, JSONObject("""{"type":"sessions_list","id":2,"sessions":[]}"""))
        assertTrue(fresh.sessions.isEmpty())
    }
    @Test fun busySnapshotKeepsLastKnownTitleAndModel() {
        val old = MobileState(sessions = listOf(MobileSession("s", "Project title", model = "model-a")))
        val state = MobileReducer.reduce(old, JSONObject("""{"type":"sessions_list","sessions":[{"session_id":"s","friendly_name":"fox","status":"running"}]}"""))
        assertEquals("Project title", state.sessions.single().name)
        assertEquals("model-a", state.sessions.single().model)
        assertTrue(state.sessions.single().isProcessing)
    }
    @Test fun mismatchedRecipientCannotBecomeAccepted() {
        val old = MobileState(deliveries = listOf(DeliveryResult(1, "a", "A", "hello", DeliveryStatus.SENDING)))
        val state = MobileReducer.reduce(old, JSONObject("""{"type":"mobile_delivery","id":1,"session_id":"wrong","status":"accepted"}"""))
        assertEquals(DeliveryStatus.UNKNOWN, state.deliveries.single().status)
    }
    @Test fun refreshedOldestTranscriptRemainsInCache() {
        val cache = (0 until 10).associate { "s$it" to listOf(TranscriptEntry("old", "assistant", "old")) }
        var state = MobileReducer.reduce(MobileState(transcripts = cache), JSONObject("""{"type":"comm_context_history","session_id":"s0","messages":[{"role":"assistant","content":"fresh"}]}"""))
        state = MobileReducer.reduce(state, JSONObject("""{"type":"comm_context_history","session_id":"s10","messages":[]}"""))
        assertEquals(10, state.transcripts.size)
        assertEquals("fresh", state.transcripts.getValue("s0").single().text)
        assertFalse(state.transcripts.containsKey("s1"))
    }
    @Test fun gatewayAddressDefaultsAndTls() {
        assertEquals(7643, GatewayAddress.parse("my-host").port)
        assertEquals(7643, GatewayAddress.parse("[::1]").port)
        assertEquals(9999, GatewayAddress.parse("[::1]:9999").port)
        assertEquals("https", GatewayAddress.parse("https://example.com").scheme)
        assertEquals(443, GatewayAddress.parse("https://example.com").port)
    }
    @Test(expected = IllegalArgumentException::class) fun gatewayRejectsCredentials() { GatewayAddress.parse("http://user:pass@host") }
    @Test(expected = IllegalArgumentException::class) fun gatewayRejectsPaths() { GatewayAddress.parse("http://host/path") }
}
