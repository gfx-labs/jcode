package dev.jcode.mobile.notifications

import dev.jcode.mobile.data.MobileSession
import org.junit.Assert.*
import org.junit.Test

class SessionTransitionsTest {
    private fun session(status: String, output: String = "", report: String = "") =
        MobileSession("s", status = status, isProcessing = status == "running", output = output, completionReport = report)
    private fun SessionTransitions.poll(status: String, output: String = "", report: String = "") =
        observe(listOf(session(status, output, report)), setOf("s")).map { it.second }

    @Test fun terminalBaselineIsSilentAndRepeatedStatusDoesNotAlert() {
        val tracker = SessionTransitions()
        assertTrue(tracker.poll("completed").isEmpty())
        assertTrue(tracker.poll("completed").isEmpty())
        tracker.poll("ready")
        tracker.poll("running")
        assertEquals(listOf(SessionAlert.COMPLETED), tracker.poll("completed"))
        assertTrue(tracker.poll("succeeded").isEmpty())
    }
    @Test fun explicitFailureAndInputTransitions() {
        val tracker = SessionTransitions()
        tracker.poll("ready")
        assertEquals(listOf(SessionAlert.FAILED), tracker.poll("failed"))
        assertTrue(tracker.poll("error").isEmpty())
        assertEquals(listOf(SessionAlert.NEEDS_INPUT), tracker.poll("needs_input"))
        assertTrue(tracker.poll("waiting_for_input").isEmpty())
    }
    @Test fun metadataBusyNeverReannouncesUnchangedTerminalStatus() {
        for (status in listOf("completed", "failed", "needs_input")) {
            val tracker = SessionTransitions()
            tracker.poll(status)
            tracker.poll("running")
            assertTrue(tracker.poll(status).isEmpty())
        }
    }
    @Test fun idleBlockedAndMissingNeverBecomeInputOrCompletion() {
        val tracker = SessionTransitions()
        tracker.poll("running")
        assertTrue(tracker.poll("idle").isEmpty())
        assertTrue(tracker.poll("blocked", report = "Need a dependency").isEmpty())
        assertTrue(tracker.observe(emptyList(), setOf("s")).isEmpty())
        assertTrue(tracker.poll("completed").isEmpty())
    }
    @Test fun metadataLockContentionWithMissingOutputNeverFinishesTurn() {
        val tracker = SessionTransitions()
        tracker.poll("running") // first snapshot skipped old assistant fallback because metadata lock was busy
        assertTrue(tracker.poll("ready", "OLD response").isEmpty())
        tracker.poll("running")
        assertTrue(tracker.poll("ready", "OLD response").isEmpty())
    }
    @Test fun initialBusyAndFirstEverOutputStayConservative() {
        val tracker = SessionTransitions()
        tracker.poll("running", "partial")
        assertTrue(tracker.poll("ready", "complete").isEmpty())
        val empty = SessionTransitions()
        empty.poll("ready")
        empty.poll("running")
        assertTrue(empty.poll("ready", "first output").isEmpty())
    }
    @Test fun genuineOutputChangeAcrossKnownIdleWorkCycleFinishesResponding() {
        val tracker = SessionTransitions()
        tracker.poll("ready", "old answer")
        tracker.poll("running")
        tracker.poll("running", "new answer partial")
        assertEquals(listOf(SessionAlert.TURN_FINISHED), tracker.poll("ready", "new answer"))
        assertTrue(tracker.poll("ready", "new answer").isEmpty())
    }
    @Test fun changingReportIsNotClaimedAsTaskSuccess() {
        val tracker = SessionTransitions()
        tracker.poll("ready", report = "old")
        assertEquals(listOf(SessionAlert.REPORT_UPDATED), tracker.poll("ready", report = "Please review"))
        assertTrue(tracker.poll("failed", report = "failure report").none { it == SessionAlert.COMPLETED })
    }
    @Test fun optOutAndReenableHaveSilentBaseline() {
        val tracker = SessionTransitions()
        tracker.poll("running")
        assertTrue(tracker.observe(listOf(session("failed")), emptySet()).isEmpty())
        assertTrue(tracker.poll("failed").isEmpty())
    }
    @Test fun snoozeBoundaryAndMute() {
        assertFalse(SessionNotificationPolicy().allows(Long.MAX_VALUE))
        assertFalse(SessionNotificationPolicy(true, 100).allows(99))
        assertTrue(SessionNotificationPolicy(true, 100).allows(100))
    }
}
