package dev.jcode.mobile.data

/** Polling/attachment does not count as work. Unknown activity sorts last. */
fun List<MobileSession>.byRecentActivity(): List<MobileSession> = sortedWith(
    compareByDescending<MobileSession> { it.pinned }
        .thenBy { it.lastActivityAgeSecs == null }
        .thenBy { it.lastActivityAgeSecs }
        .thenBy { it.id }
)

/** Keep the distinguishing directory names visible on narrow phone cards. */
fun directorySummary(path: String): String {
    val parts = path.replace('\\', '/').split('/').filter { it.isNotEmpty() }
    return if (parts.size > 3) "…/" + parts.takeLast(3).joinToString("/") else path
}
