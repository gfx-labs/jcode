package dev.jcode.mobile.data
import org.junit.Assert.*
import org.junit.Test
class SessionFiltersTest {
    private val fresh = MobileSession("fresh", "Gateway", isProcessing = true, workingDirectory = "/repos/jcode", lastActivityAgeSecs = 5)
    private val old = MobileSession("old", "Docs", workingDirectory = "/repos/docs", lastActivityAgeSecs = 90000)
    @Test fun searchesPathsCaseInsensitivelyAndSortsRecentFirst() {
        assertEquals(listOf(fresh, old), visibleSessions(listOf(old, fresh), " /REPOS ", false))
        assertEquals(listOf(old), visibleSessions(listOf(old, fresh), "docs", false))
    }
    @Test fun activeFilterComposesWithSearch() {
        assertEquals(listOf(fresh), visibleSessions(listOf(old, fresh), "", true))
        assertTrue(visibleSessions(listOf(old, fresh), "docs", true).isEmpty())
    }
    @Test fun activityLabelsCoverAllScales() {
        assertEquals("Now", activityLabel(5)); assertEquals("1m", activityLabel(60))
        assertEquals("1h", activityLabel(3600)); assertEquals("1d", activityLabel(86400))
    }
}
