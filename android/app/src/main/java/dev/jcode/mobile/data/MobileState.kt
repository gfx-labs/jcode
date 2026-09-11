package dev.jcode.mobile.data

enum class ConnectionStatus { DISCONNECTED, PAIRING, CONNECTING, CONNECTED, RECONNECTING, DEMO }
enum class MessageReceipt(val label: String) { SENDING("Sending…"), SENT("Sent"), READ("Read"), FAILED("Not sent"), UNKNOWN("Unconfirmed") }
enum class DeliveryStatus { SENDING, ACCEPTED, REJECTED, UNKNOWN }
data class MobileTodo(val content: String, val status: String)
data class MobileSession(
    val id: String, val name: String = id, val status: String = "unknown",
    val detail: String = "", val taskLabel: String = "", val role: String = "agent",
    val parentId: String? = null, val model: String = "", val isProcessing: Boolean = false,
    val currentTool: String = "", val todos: List<MobileTodo> = emptyList(),
    val todosCompleted: Int = 0, val todosTotal: Int = 0, val output: String = "",
    val completionReport: String = "", val tokens: Long = 0,
    val workingDirectory: String = "", val lastActivityAgeSecs: Long? = null, val pinned: Boolean = false, val agentName: String = "", val providerName: String = "",
)
data class ToolEntry(val id: String, val name: String, val input: String = "", val output: String = "", val status: String = "running", val error: String? = null)
data class TranscriptEntry(val id: String, val role: String, val text: String = "", val reasoning: String = "", val tools: List<ToolEntry> = emptyList(), val isStreaming: Boolean = false, val displayRole: String? = null, val messageId: String? = null, val timestampUnixMs: Long? = null)
data class DeliveryResult(val requestId: Long, val sessionId: String, val sessionName: String, val text: String, val status: DeliveryStatus, val detail: String = "",
    // Null means history was not loaded at send, so matching would risk consuming an old message.
    val historyBaseline: List<String>? = null,
    val insertionIndex: Int? = null,
    val outputAtSend: String? = null,
    val matchedHistoryIndex: Int? = null,
    val matchedHistoryAnchor: List<String>? = null,
    val read: Boolean = false,
    val messageId: String? = null,
    // Captured once at the send action, never during rendering or history polling.
    val sentAtUnixMs: Long? = null,
) {
    val localId: String get() = "local:$sessionId:$requestId"
    val receipt: MessageReceipt get() = when {
        read -> MessageReceipt.READ
        matchedHistoryIndex != null -> MessageReceipt.SENT
        status == DeliveryStatus.SENDING -> MessageReceipt.SENDING
        status == DeliveryStatus.ACCEPTED -> MessageReceipt.SENT
        status == DeliveryStatus.REJECTED -> MessageReceipt.FAILED
        else -> MessageReceipt.UNKNOWN
    }
}
data class MobileState(
    val connection: ConnectionStatus = ConnectionStatus.DISCONNECTED, val host: String = "",
    val serverName: String = "Jcode", val sessions: List<MobileSession> = emptyList(),
    val selectedSessionId: String? = null, val transcripts: Map<String, List<TranscriptEntry>> = emptyMap(),
    val deliveries: List<DeliveryResult> = emptyList(), val error: String? = null, val isDemo: Boolean = false, val lastUpdatedMillis: Long = 0,
    val openThreadRequest: Long = 0L,
) {
    val selectedSession: MobileSession? get() = sessions.firstOrNull { it.id == selectedSessionId }
}
