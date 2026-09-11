package dev.jcode.mobile.data

import org.json.JSONObject
import org.junit.Assert.*
import org.junit.Test

class SessionPresentationTest {
    @Test fun decodesWorkingDirectoryAndRecentActivity() {
        val session = WireCodec.session(JSONObject("""{"session_id":"s","working_dir":"/home/me/projects/jcode","last_activity_age_secs":12}"""))
        assertEquals("/home/me/projects/jcode", session.workingDirectory)
        assertEquals(12L, session.lastActivityAgeSecs)
    }

    @Test fun unavailableActivityIsNotMistakenForJustNow() {
        listOf("null", "-1", "\"bad\"").forEach { value ->
            val session = WireCodec.session(JSONObject("""{"session_id":"s","working_dir":null,"last_activity_age_secs":$value}"""))
            assertEquals("", session.workingDirectory)
            assertNull(session.lastActivityAgeSecs)
        }
        assertNull(WireCodec.session(JSONObject("""{"session_id":"old-daemon"}""")).lastActivityAgeSecs)
    }

    @Test fun sortsByActivityNotNameStatusOrParentGrouping() {
        val sessions = listOf(
            MobileSession("stale-parent", status = "running", lastActivityAgeSecs = 900),
            MobileSession("unknown", isProcessing = true),
            MobileSession("recent-child", parentId = "stale-parent", lastActivityAgeSecs = 0),
            MobileSession("recent-ready", status = "ready", lastActivityAgeSecs = 2),
            MobileSession("oldest-known", lastActivityAgeSecs = Long.MAX_VALUE),
        )
        assertEquals(listOf("recent-child", "recent-ready", "stale-parent", "oldest-known", "unknown"), sessions.byRecentActivity().map { it.id })
    }

    @Test fun equalActivityHasDeterministicOrder() {
        val sessions = listOf(MobileSession("b", lastActivityAgeSecs = 5), MobileSession("a", lastActivityAgeSecs = 5))
        assertEquals(listOf("a", "b"), sessions.byRecentActivity().map { it.id })
        assertEquals(sessions.byRecentActivity(), sessions.reversed().byRecentActivity())
    }

    @Test fun refreshResortsAndKeepsDirectoryWhenBusySnapshotOmitsIt() {
        val initial = MobileState(sessions = listOf(MobileSession("a", workingDirectory = "/projects/a", lastActivityAgeSecs = 0), MobileSession("b", lastActivityAgeSecs = 30)))
        val fresh = MobileReducer.reduce(initial, JSONObject("""{"type":"sessions_list","sessions":[{"session_id":"a","last_activity_age_secs":20},{"session_id":"b","working_dir":"/projects/b","last_activity_age_secs":0}]}"""))
        assertEquals(listOf("b", "a"), fresh.sessions.map { it.id })
        assertEquals("/projects/a", fresh.sessions.last().workingDirectory)
        assertEquals("/projects/b", fresh.sessions.first().workingDirectory)
        val changed = MobileReducer.reduce(fresh, JSONObject("""{"type":"sessions_list","sessions":[{"session_id":"a","working_dir":"/new/a","last_activity_age_secs":0}]}"""))
        assertEquals("/new/a", changed.sessions.single().workingDirectory)
    }

    @Test fun pathSummaryKeepsDistinguishingDirectoryNames() {
        assertEquals("…/gfx-labs/jcode/gfx", directorySummary("/home/me/Projects/repos/github.com/gfx-labs/jcode/gfx"))
        assertEquals("/projects/jcode", directorySummary("/projects/jcode"))
        assertEquals("/", directorySummary("/"))
        assertEquals("", directorySummary(""))
        assertEquals("…/repos/jcode/mobile", directorySummary("C:\\Users\\me\\repos\\jcode\\mobile"))
        assertEquals("…/work/日本語/mobile", directorySummary("/home/me/work/日本語/mobile"))
    }
}
