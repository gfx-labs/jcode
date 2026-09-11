package dev.jcode.mobile.data

fun visibleSessions(sessions: List<MobileSession>, query: String, activeOnly: Boolean): List<MobileSession> {
    val needle = query.trim()
    return sessions.filter { session ->
        (!activeOnly || session.isProcessing) &&
            (needle.isEmpty() || listOf(session.name, session.workingDirectory, session.taskLabel, session.detail)
                .any { it.contains(needle, ignoreCase = true) })
    }.byRecentActivity()
}

fun activityLabel(age: Long?): String = when {
    age == null -> "—"
    age < 60 -> "Now"
    age < 3600 -> "${age / 60}m"
    age < 86400 -> "${age / 3600}h"
    else -> "${age / 86400}d"
}
