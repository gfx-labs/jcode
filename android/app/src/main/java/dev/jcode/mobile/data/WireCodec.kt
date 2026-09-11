package dev.jcode.mobile.data

import org.json.JSONArray
import org.json.JSONObject

/** One websocket text frame may contain several newline-delimited protocol records. */
object WireCodec {
    fun decodeFrame(text: String): List<JSONObject> = text.lineSequence().filter { it.isNotBlank() }.map {
        JSONObject(it).also { event -> require(event.opt("type") is String) { "Missing event type" } }
    }.toList()
    fun request(type: String, id: Long, vararg fields: Pair<String, Any>): String = JSONObject().put("type", type).put("id", id).apply {
        fields.forEach { (key, value) -> put(key, value) }
    }.toString() + "\n"

    fun session(json: JSONObject): MobileSession {
        val id = json.text("session_id")
        val runtime = json.optJSONObject("runtime")
        val activity = json.optJSONObject("activity")
        val progress = json.optJSONArray("todo_progress")
        return MobileSession(id, json.text("title").ifBlank { json.text("friendly_name").ifBlank { id } },
            json.text("status").ifBlank { "unknown" }, json.text("detail"), json.text("task_label"),
            json.text("role").ifBlank { "agent" }, json.nullableText("report_back_to_session_id"),
            json.text("provider_model").ifBlank { runtime?.text("model").orEmpty() },
            activity?.optBoolean("is_processing") ?: (json.text("status") == "running"),
            activity?.text("current_tool_name").orEmpty(),
            json.objects("todo_items").map { MobileTodo(it.text("content"), it.text("status")) },
            json.optInt("todos_completed", progress?.optInt(0) ?: 0),
            json.optInt("todos_total", progress?.optInt(1) ?: 0), json.text("output_tail"),
            json.text("latest_completion_report"), json.optLong("cumulative_total_tokens"),
            workingDirectory = json.text("working_dir"), agentName = json.text("friendly_name"), providerName = json.text("provider_name"),
            lastActivityAgeSecs = (json.opt("last_activity_age_secs") as? Number)?.toLong()?.takeIf { it >= 0 })
    }

    fun history(json: JSONObject): List<TranscriptEntry> {
        val entries = mutableListOf<TranscriptEntry>()
        json.objects("messages").takeLast(1000).forEachIndexed { index, message ->
            val tool = message.optJSONObject("tool_data")
            if (tool != null) {
                val entry = ToolEntry(tool.text("id"), tool.text("name"), tool.text("input"), tool.text("output"),
                    if (tool.nullableText("error") == null) "succeeded" else "failed", tool.nullableText("error"))
                // Rust history separates tool records from assistant tool-call messages.
                val owner = entries.indexOfLast { it.role == "assistant" }
                if (owner >= 0) entries[owner] = entries[owner].copy(tools = entries[owner].tools + entry)
                else entries += TranscriptEntry("history-$index", "tool", message.text("content"), tools = listOf(entry), timestampUnixMs = message.timestampUnixMs())
            } else entries += TranscriptEntry("history-$index", message.text("role"), message.text("content"), displayRole = message.nullableText("display_role"), messageId = message.nullableText("message_id"), timestampUnixMs = message.timestampUnixMs())
        }
        return entries
    }
}

/** Only integral wire values are timestamps. Missing, malformed and legacy values stay unknown. */
internal fun JSONObject.timestampUnixMs(): Long? = when (val value = opt("timestamp_unix_ms")) {
    is Long -> value.takeIf { it in -62135596800000L..253402300799999L } // Calendar years 1–9999.
    is Int -> value.toLong()
    else -> null
}

internal fun JSONObject.text(key: String): String = opt(key) as? String ?: ""
internal fun JSONObject.nullableText(key: String): String? = (opt(key) as? String)?.takeIf { it.isNotEmpty() }
internal fun JSONObject.objects(key: String): List<JSONObject> = optJSONArray(key)?.let { array ->
    (0 until array.length()).mapNotNull { array.optJSONObject(it) }
}.orEmpty()

/** Pure reducer. Unknown future events are deliberately ignored. */
object MobileReducer {
    fun disconnected(state: MobileState): MobileState = state.copy(deliveries = state.deliveries.map {
        if (it.status == DeliveryStatus.SENDING) it.copy(status = DeliveryStatus.UNKNOWN, detail = "Connection ended before confirmation. Not resent.") else it
    })

    fun reduce(state: MobileState, event: JSONObject, sessionId: String? = null): MobileState {
        when (event.text("type")) {
            "sessions_list" -> return state.copy(sessions = event.objects("sessions").map { json ->
                val parsed = WireCodec.session(json)
                val previous = state.sessions.firstOrNull { it.id == parsed.id }
                parsed.copy(
                    name = if (json.nullableText("title") == null && previous != null) previous.name else parsed.name,
                    model = parsed.model.ifBlank { previous?.model.orEmpty() },
                    providerName = parsed.providerName.ifBlank { previous?.providerName.orEmpty() },
                    workingDirectory = parsed.workingDirectory.ifBlank { previous?.workingDirectory.orEmpty() },
                )
            }.byRecentActivity(), serverName = event.text("server_name").ifBlank { state.serverName })
            "mobile_delivery" -> return state.copy(deliveries = state.deliveries.map {
                if (it.requestId != event.optLong("id")) it else if (it.sessionId != event.text("session_id")) it.copy(status = DeliveryStatus.UNKNOWN, detail = "Gateway recipient mismatch. Not resent.") else it.copy(
                    status = when (event.text("status")) { "accepted" -> DeliveryStatus.ACCEPTED; "rejected" -> DeliveryStatus.REJECTED; else -> DeliveryStatus.UNKNOWN },
                    detail = event.text("message"), messageId = event.nullableText("message_id") ?: it.messageId)
            })
            "history", "comm_context_history" -> {
                val id = event.text("session_id").ifBlank { sessionId.orEmpty() }
                if (id.isBlank()) return state
                val history = WireCodec.history(event)
                return state.copy(transcripts = ((state.transcripts - id) + (id to history)).entries.toList().takeLast(10).associate { it.toPair() },
                    deliveries = reconcileDeliveries(id, history, state.deliveries))
            }
            "error" -> {
                val id = event.optLong("id", -1)
                val delivery = state.deliveries.any { it.requestId == id }
                return if (delivery) state.copy(deliveries = state.deliveries.map {
                    if (it.requestId == id) it.copy(status = DeliveryStatus.REJECTED, detail = event.text("message")) else it
                }) else state.copy(error = event.text("message"))
            }
        }
        val id = sessionId ?: return state
        var entries = state.transcripts[id].orEmpty()
        fun updateStreaming(change: (TranscriptEntry) -> TranscriptEntry) {
            val last = entries.lastOrNull()
            if (last?.role == "assistant" && last.isStreaming) entries = entries.dropLast(1) + change(last)
            else entries = entries + change(TranscriptEntry("live-${entries.size}", "assistant", isStreaming = true))
        }
        when (event.text("type")) {
            "text_delta" -> updateStreaming { it.copy(text = it.text + event.text("text")) }
            "text_replace" -> updateStreaming { it.copy(text = event.text("text")) }
            "reasoning_delta" -> updateStreaming { it.copy(reasoning = it.reasoning + event.text("text")) }
            "tool_start" -> updateStreaming { it.copy(tools = it.tools + ToolEntry(event.text("id"), event.text("name"))) }
            "tool_input" -> updateStreaming { it.copy(tools = it.tools.mapIndexed { index, tool -> if (index == it.tools.lastIndex) tool.copy(input = tool.input + event.text("delta")) else tool }) }
            "tool_done" -> updateStreaming { it.copy(tools = it.tools.map { tool -> if (tool.id == event.text("id")) tool.copy(output = event.text("output"), error = event.nullableText("error"), status = if (event.nullableText("error") == null) "succeeded" else "failed") else tool }) }
            "retry_rollback" -> entries = entries.filterNot { it.isStreaming }
            "done", "message_end", "interrupted" -> entries = entries.map { it.copy(isStreaming = false) }
            else -> return state
        }
        return state.copy(transcripts = state.transcripts + (id to entries))
    }
}
