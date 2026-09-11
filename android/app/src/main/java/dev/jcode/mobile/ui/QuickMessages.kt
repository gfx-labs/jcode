package dev.jcode.mobile.ui

import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.widthIn
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.outlined.ChatBubbleOutline
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.Modifier
import androidx.compose.ui.unit.dp

internal data class QuickMessage(val title: String, val hint: String, val message: String)

internal val quickMessages = listOf(
    QuickMessage("Status", "Progress, blockers, next step", "Give me a concise status update on the current task: what is completed, what is in progress (including sub-agents), any blockers, and the next concrete step. Distinguish verified results from assumptions. Do not restart or duplicate ongoing work."),
    QuickMessage("Continue", "Resume the agreed work", "Continue the current agreed task from where you left off. Check existing work and running sub-agents first so nothing is duplicated. Complete the remaining steps and verify the result. Stay within the agreed scope and permissions; ask only if a decision or missing information truly blocks progress."),
    QuickMessage("What's blocking?", "Find the smallest way forward", "Identify what is currently blocking progress, if anything. Explain what you tried, the evidence for the blocker, and the smallest safe next step. If you need something from me, ask one specific question with your recommended option. If nothing is blocked, say so and continue the agreed work."),
    QuickMessage("Verify it", "Check the real user workflow", "Verify the current changes against the original requirements using the real user workflow and relevant existing checks. Check likely regressions and integration boundaries, and fix issues within the agreed scope. Report what passed, what failed, and what could not be tested. Do not treat a successful build alone as proof or add unnecessary tests just to record validation."),
    QuickMessage("Team check-in", "Coordinate existing agents", "Check in with the existing sub-agents working on this task. Summarize each agent's responsibility, progress, blockers, and next step. Resolve overlapping work or conflicting assumptions, and continue coordinating toward the agreed goal. Do not spawn additional agents just for this update."),
    QuickMessage("Review the plan", "Reassess before doing more", "Briefly restate the goal and review the current approach against it. Identify unnecessary work, unresolved assumptions, or a simpler alternative. Recommend the next steps, but do not expand scope or start a new approach without my approval."),
    QuickMessage("Pause safely", "Stop after the current safe step", "Pause this task at the next safe stopping point. Do not start new work. Ask existing sub-agents on this task to pause safely too. Preserve current changes and report any operations still running, incomplete work, and what is needed to resume. Do not discard changes or terminate a critical operation abruptly."),
    QuickMessage("Handoff summary", "Decisions, changes, remaining work", "Prepare a concise handoff for this task: goal, key decisions, files or artifacts changed, verification performed, remaining work, blockers, and the exact next step. Include relevant existing agent or task identifiers. Clearly separate completed work from plans, and do not expose credentials or other secrets.")
)

@Composable
internal fun QuickMessages(draft: String, onDraft: (String) -> Unit) {
    var expanded by remember { mutableStateOf(false) }
    Box {
        TextButton(onClick = { expanded = true }) {
            Icon(Icons.Outlined.ChatBubbleOutline, null)
            Text(" Quick messages")
        }
        DropdownMenu(expanded, onDismissRequest = { expanded = false }, modifier = Modifier.widthIn(max = 320.dp).heightIn(max = 360.dp)) {
            DropdownMenuItem(text = { Text("Adds an editable draft. Tap Send when ready.", style = MaterialTheme.typography.labelSmall) }, enabled = false, onClick = {})
            quickMessages.forEach { template ->
                DropdownMenuItem(text = {
                    androidx.compose.foundation.layout.Column {
                        Text(template.title)
                        Text(template.hint, style = MaterialTheme.typography.bodySmall, color = Muted)
                    }
                }, onClick = {
                    onDraft(if (draft.isBlank()) template.message else "$draft\n\n${template.message}")
                    expanded = false
                })
            }
        }
    }
}
