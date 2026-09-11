package dev.jcode.mobile.data

/** Provider `user` is also used to inject notifications. It is not authorship. */
data class MessageAttribution(val label: String, val isHuman: Boolean = false, val sender: String? = null, val body: String? = null, val isEvent: Boolean = false)

private val timestampEnvelope = Regex("^\\[\\d{4}-\\d{2}-\\d{2}T[^]\\r\\n]+]\\s+")
private val agentEnvelope = Regex("^(DM|broadcast|#[^\\s:]+) from ([^:\\r\\n]+):(?:\\s|$)")
private val backgroundEnvelope = Regex("^\\*\\*(Background task(?: started| progress| stalled)?)\\*\\*(?:\\s|$)")

/**
 * Honor explicit display roles first. Legacy histories lack notification metadata,
 * so recognize only anchored daemon-generated envelopes, never body substrings.
 * Keep message text intact. Classification is presentation, not authentication.
 */
fun messageAttribution(entry: TranscriptEntry): MessageAttribution {
    val role = entry.displayRole?.takeIf { it.isNotBlank() } ?: entry.role
    if (role == "user" && (entry.messageId?.startsWith("mobile:") == true || entry.id.startsWith("local:"))) return MessageAttribution("You", isHuman = true)
    if (role == "assistant") return MessageAttribution("Jcode")
    if (role == "tool") return MessageAttribution("Tool output")
    val text = entry.text.trimStart().replaceFirst(timestampEnvelope, "")
    if (role in setOf("user", "system", "swarm", "notification", "background_task")) {
        agentEnvelope.find(text)?.let {
            val scope = it.groupValues[1]
            val sender = it.groupValues[2].trim()
            return MessageAttribution(when (scope) {
                "DM" -> "DM · $sender"
                "broadcast" -> "Broadcast · $sender"
                else -> "$scope · $sender"
            }, sender = sender, body = text.substring(it.range.last + 1).trimStart())
        }
        backgroundEnvelope.find(text)?.let { return MessageAttribution(it.groupValues[1], isEvent = true) }
        if (text.startsWith("[Scheduled task]")) return MessageAttribution("Scheduled task", isEvent = true)
        if (text.startsWith("[auto] ") ||
            (text.startsWith("You have ") && text.contains(" incomplete todo") && text.trimEnd().endsWith("update the todo tool.")))
            return MessageAttribution("Automation", isEvent = true)
        if (text.startsWith("<system-reminder>") || text.startsWith("<system-notification>")) return MessageAttribution("System", isEvent = true)
        if (text.startsWith("Shell command · ")) return MessageAttribution("Shell output", isEvent = true)
    }
    return when (role) {
        "user" -> MessageAttribution("You", isHuman = true)
        "background_task" -> MessageAttribution("Background task", isEvent = true)
        "system" -> MessageAttribution("System", isEvent = true)
        "swarm" -> MessageAttribution("Agent notification", isEvent = true)
        "notification" -> MessageAttribution("Notification", isEvent = true)
        else -> MessageAttribution(role.replace('_', ' ').replaceFirstChar { it.uppercase() }.ifBlank { "Event" }, isEvent = true)
    }
}
