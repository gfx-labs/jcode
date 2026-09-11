//! Owner-level control requests on the already trusted local/gateway transport.
//! None of these paths registers an attachment or changes a TUI's ownership.
use super::{ClientConnectionInfo, SessionAgents, SwarmMember};
use crate::protocol::{LiveSessionInfo, ServerEvent, SwarmTodoItem};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

pub(super) async fn live_session_snapshots(
    sessions: &SessionAgents,
    swarm_members: &Arc<RwLock<HashMap<String, SwarmMember>>>,
    client_connections: &Arc<RwLock<HashMap<String, ClientConnectionInfo>>>,
) -> Vec<LiveSessionInfo> {
    // The global agent map is authoritative. Persisted/stopped swarm records
    // alone must not make a session appear live. Clone handles, never wait for
    // a busy agent or hold the global map lock across snapshot collection.
    let agents: Vec<_> = sessions
        .read()
        .await
        .iter()
        .map(|(id, agent)| (id.clone(), Arc::clone(agent)))
        .collect();
    let mut result = Vec::with_capacity(agents.len());
    for (session_id, agent) in agents {
        let mut snapshot = {
            let members = swarm_members.read().await;
            let member = members.get(&session_id);
            LiveSessionInfo {
                session_id: session_id.clone(),
                last_activity_age_secs: crate::session_metrics::last_activity_age_secs(&session_id),
                status: member
                    .map(|m| m.status.clone())
                    .unwrap_or_else(|| "ready".into()),
                role: member.map(|m| m.role.clone()),
                latest_completion_report: member.and_then(|m| m.latest_completion_report.clone()),
                friendly_name: member.and_then(|m| m.friendly_name.clone()),
                detail: member.and_then(|m| m.detail.clone()),
                task_label: member.and_then(|m| m.task_label.clone()),
                report_back_to_session_id: member.and_then(|m| m.report_back_to_session_id.clone()),
                provider_model: member.and_then(|m| m.runtime.model.clone()),
                provider_name: member.and_then(|m| m.runtime.provider.clone()),
                working_dir: member.and_then(|m| {
                    m.working_dir
                        .as_ref()
                        .map(|p| p.to_string_lossy().into_owned())
                }),
                swarm_id: member.and_then(|m| m.swarm_id.clone()),
                output_tail: member.and_then(|m| m.output_tail.clone()),
                todo_items: member.map(|m| m.todo_items.clone()).unwrap_or_default(),
                todos_completed: member.and_then(|m| m.todo_progress.map(|p| p.0 as usize)),
                todos_total: member.and_then(|m| m.todo_progress.map(|p| p.1 as usize)),
                ..Default::default()
            }
        };
        let busy = if let Ok(agent) = agent.try_lock() {
            snapshot.provider_model = Some(agent.provider_model());
            snapshot.provider_name = Some(agent.provider_name());
            snapshot.working_dir = agent
                .working_dir()
                .map(str::to_owned)
                .or(snapshot.working_dir);
            snapshot.friendly_name = agent
                .session_short_name()
                .map(str::to_owned)
                .or(snapshot.friendly_name);
            let session = agent.session_for_split();
            // Restore/save touches updated_at without conversation activity.
            // Use durable substantive message timestamps instead, ignoring
            // synthetic context/reminder messages that restore may inject.
            if snapshot.last_activity_age_secs.is_none() {
                let last_activity = session
                    .messages
                    .iter()
                    .filter(|message| {
                        message.display_role.is_none()
                            && !message.content.iter().any(|block| {
                                matches!(block, crate::message::ContentBlock::Text { text, .. }
                                    if text.trim_start().starts_with("<system-reminder>"))
                            })
                    })
                    .filter_map(|message| message.timestamp)
                    .max()
                    .unwrap_or(session.created_at);
                snapshot.last_activity_age_secs = Some(
                    (chrono::Utc::now() - last_activity).num_seconds().max(0) as u64,
                );
            }
            snapshot.title = session
                .custom_title
                .clone()
                .or_else(|| session.title.clone());
            if snapshot.output_tail.is_none() {
                snapshot.output_tail = agent
                    .latest_assistant_text_after(0)
                    .map(|text| bounded_tail(&text));
            }
            false
        } else {
            true
        };
        snapshot.activity = super::comm_sync::live_activity_snapshot(
            &*client_connections.read().await,
            &session_id,
            busy || snapshot.status == "running",
        );
        if snapshot.activity.as_ref().is_some_and(|a| a.is_processing) {
            snapshot.status = "running".into();
        }
        if let Ok(todos) = crate::todo::load_todos(&session_id) {
            if !todos.is_empty() {
                snapshot.todos_completed =
                    Some(todos.iter().filter(|t| t.status == "completed").count());
                snapshot.todos_total = Some(todos.len());
                // Keep polling payloads bounded while retaining full counts.
                snapshot.todo_items = todos
                    .into_iter()
                    .take(32)
                    .map(|t| SwarmTodoItem {
                        content: t.content,
                        status: t.status,
                        tool_intents: Vec::new(),
                    })
                    .collect();
            }
        }
        result.push(snapshot);
    }
    result.sort_by(|a, b| {
        a.last_activity_age_secs
            .is_none()
            .cmp(&b.last_activity_age_secs.is_none())
            .then_with(|| a.last_activity_age_secs.cmp(&b.last_activity_age_secs))
            .then_with(|| a.session_id.cmp(&b.session_id))
    });
    result
}

pub(super) fn bounded_tail(text: &str) -> String {
    const MAX_BYTES: usize = 4096;
    let mut start = text.len().saturating_sub(MAX_BYTES);
    while !text.is_char_boundary(start) {
        start += 1;
    }
    text[start..].to_owned()
}

pub(super) async fn handle_mobile_message(
    id: u64,
    session_id: String,
    content: String,
    ctx: super::client_actions::NotifySessionContext<'_>,
) {
    // This endpoint represents the authenticated transport owner, never a
    // claimed from_session. Unlike notify_session, inject User, not System,
    // content and do not fan out a second wake notification that could replay it.
    let mobile_id = *uuid::Uuid::new_v4().as_bytes();
    let source = jcode_agent_runtime::SoftInterruptSource::MobileUser(mobile_id);
    let agent = ctx.sessions.read().await.get(&session_id).cloned();
    let accepted = if content.trim().is_empty() || agent.is_none() {
        false
    } else if let Ok(agent) = Arc::clone(agent.as_ref().unwrap()).try_lock_owned() {
        super::live_turn::spawn_tracked_mobile_turn(
            &session_id,
            agent,
            content.clone(),
            mobile_id,
            Some(super::truncate_detail(&content, 120)),
            super::live_turn::LiveTurnSwarmContext::new(
                ctx.swarm_members,
                ctx.swarms_by_id,
                ctx.event_history,
                ctx.event_counter,
                ctx.swarm_event_tx,
            ),
        )
        .await;
        true
    } else {
        let queue = ctx
            .soft_interrupt_queues
            .read()
            .await
            .get(&session_id)
            .cloned();
        let queued = queue
            .as_ref()
            .map(|queue| {
                super::state::enqueue_soft_interrupt(
                    queue,
                    content.clone(),
                    Vec::new(),
                    false,
                    source,
                )
            })
            .unwrap_or(true);
        if queued {
            // A busy mutex may be a short metadata read, or the current turn
            // may already have passed its final interrupt checkpoint. Reserve
            // the agent after that work finishes and wake only if this queued
            // user message was not consumed. Removing a queue entry atomically
            // prevents duplicate delivery, including identical concurrent sends.
            let agent = agent.unwrap();
            let sessions = Arc::clone(ctx.sessions);
            let target = session_id.clone();
            let swarm = super::live_turn::LiveTurnSwarmContext::new(
                ctx.swarm_members,
                ctx.swarms_by_id,
                ctx.event_history,
                ctx.event_counter,
                ctx.swarm_event_tx,
            );
            tokio::spawn(async move {
                let guard = Arc::clone(&agent).lock_owned().await;
                let still_live = sessions
                    .read()
                    .await
                    .get(&target)
                    .is_some_and(|current| Arc::ptr_eq(current, &agent));
                if !still_live {
                    return;
                }
                let pending = if let Some(queue) = queue {
                    queue.lock().ok().and_then(|mut entries| {
                        entries
                            .iter()
                            .position(|entry| entry.source == source)
                            .map(|index| entries.remove(index).content)
                    })
                } else {
                    // An unregistered live agent has no interrupt queue to
                    // inject into. Deliver directly once its mutex is free.
                    Some(content.clone())
                };
                if let Some(pending) = pending {
                    guard.persist_soft_interrupt_snapshot();
                    super::live_turn::spawn_tracked_mobile_turn(
                        &target,
                        guard,
                        pending,
                        mobile_id,
                        Some(super::truncate_detail(&content, 120)),
                        swarm,
                    )
                    .await;
                }
            });
        }
        queued
    };
    let _ = ctx.client_event_tx.send(ServerEvent::MobileDelivery {
        id,
        session_id,
        message_id: accepted.then(|| format!("mobile:{}", uuid::Uuid::from_bytes(mobile_id))),
        status: if accepted { "accepted" } else { "rejected" }.into(),
        message: if accepted {
            "Message received or queued. Model completion is not implied."
        } else {
            "Message is empty or the target session is not currently live."
        }
        .into(),
    });
}
