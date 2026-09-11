package dev.jcode.mobile.notifications

import dev.jcode.mobile.data.MobileSession
import java.security.MessageDigest

/** Missing sessions, generic idle and blocked never imply a request for input. */
enum class SessionAlert(val title: String) {
    COMPLETED("Session completed"), TURN_FINISHED("Session finished responding"),
    REPORT_UPDATED("Session report updated"), FAILED("Session failed"), NEEDS_INPUT("Session needs input")
}

class SessionTransitions {
    private data class Snapshot(val status: String, val working: Boolean, val report: String?, val output: String?)
    private data class WorkCycle(val outputBefore: String?, val outputChanged: Boolean)
    private val previous = mutableMapOf<String, Snapshot>()
    private val work = mutableMapOf<String, WorkCycle>()
    private val authoritative = mutableMapOf<String, String>()

    fun observe(sessions: List<MobileSession>, watchedIds: Set<String>): List<Pair<String, SessionAlert>> {
        val visible = sessions.filter { it.id in watchedIds }.distinctBy { it.id }
        val ids = visible.map { it.id }.toSet()
        previous.keys.retainAll(ids)
        work.keys.retainAll(ids)
        authoritative.keys.retainAll(ids)
        return visible.mapNotNull { session ->
            val status = session.status.lowercase()
            val next = Snapshot(status, session.isProcessing || status == "running", fingerprint(session.completionReport), fingerprint(session.output))
            val priorStatus = authoritative[session.id]
            val statusGroup = when (status) { in COMPLETED -> "completed"; in FAILED -> "failed"; in INPUT -> "needs_input"; else -> status }
            if (!next.working) authoritative[session.id] = statusGroup
            val old = previous.put(session.id, next) ?: return@mapNotNull null // Silent baseline, including new opt-ins.
            var cycle = work[session.id]
            if (!old.working && next.working && old.output != null) cycle = WorkCycle(old.output, false)
            if ((old.working || next.working) && cycle != null) {
                cycle = cycle.let {
                    it.copy(outputChanged = it.outputChanged || (next.output != null && next.output != it.outputBefore))
                }
                if (next.working) work[session.id] = cycle else work.remove(session.id)
            } else work.remove(session.id)
            val alert = when {
                next.status in FAILED && priorStatus != null && (priorStatus != "failed" || cycle?.outputChanged == true) -> SessionAlert.FAILED
                next.status in INPUT && priorStatus != null && priorStatus != "needs_input" -> SessionAlert.NEEDS_INPUT
                next.working -> null
                next.status in COMPLETED && priorStatus != null && (priorStatus != "completed" || cycle?.outputChanged == true) -> SessionAlert.COMPLETED
                // A changing report may itself be blocked or ready, not a successful task result.
                next.status in READY && next.report != null && next.report != old.report -> SessionAlert.REPORT_UPDATED
                // Daemon lock contention hides fallback output. Require a known nonempty idle baseline
                // from before the work cycle, not output first appearing after an initial busy snapshot.
                old.working && next.status in READY && cycle?.outputChanged == true -> SessionAlert.TURN_FINISHED
                else -> null
            }
            alert?.let { session.id to it }
        }
    }

    private fun fingerprint(text: String): String? = text.takeIf { it.isNotBlank() }?.let {
        MessageDigest.getInstance("SHA-256").digest(it.toByteArray(Charsets.UTF_8)).joinToString("") { byte -> "%02x".format(byte) }
    }

    companion object {
        private val INPUT = setOf("needs_input", "waiting_for_input", "awaiting_input", "needs input")
        private val READY = setOf("ready", "idle")
        private val COMPLETED = setOf("completed", "succeeded")
        private val FAILED = setOf("failed", "error")
    }
}
