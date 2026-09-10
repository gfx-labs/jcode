use crate::agent_instructions::{GeneratedAgentInstructions, generate_agent_instructions};
use crate::protocol::ServerEvent;
use crate::provider::Provider;
use anyhow::Result;
use std::sync::Arc;

#[derive(Default)]
pub(super) struct InstructionJob {
    active: Option<ActiveJob>,
}

struct ActiveJob {
    id: u64,
    session: String,
    model: String,
    provider_name: String,
    started: std::time::Instant,
    task: tokio::task::JoinHandle<Result<GeneratedAgentInstructions>>,
}

impl InstructionJob {
    pub(super) fn is_running(&self) -> bool {
        self.active.is_some()
    }

    pub(super) fn start(
        &mut self,
        id: u64,
        session: &str,
        provider: Arc<dyn Provider>,
        description: String,
        purpose: String,
        mode: String,
    ) -> Result<()> {
        anyhow::ensure!(
            !self.is_running(),
            "An instruction generation is already running on this connection"
        );
        let model = provider.model();
        let provider_name = provider.display_name();
        crate::logging::info(&format!(
            "Agent instruction generation started id={id} session={session}"
        ));
        self.active = Some(ActiveJob {
            id,
            session: session.into(),
            model,
            provider_name,
            started: std::time::Instant::now(),
            task: tokio::spawn(generate_agent_instructions(
                provider,
                description,
                purpose,
                mode,
            )),
        });
        Ok(())
    }

    pub(super) fn cancel(&mut self, id: u64) -> bool {
        if self.active.as_ref().is_some_and(|job| job.id == id) {
            self.clear();
            true
        } else {
            false
        }
    }

    pub(super) fn clear(&mut self) {
        if let Some(job) = self.active.take() {
            job.task.abort();
            crate::logging::info(&format!(
                "Agent instruction generation cancelled id={} session={} elapsed_ms={}",
                job.id,
                job.session,
                job.started.elapsed().as_millis()
            ));
        }
    }

    pub(super) async fn next(&mut self, session: &str) -> Option<ServerEvent> {
        let job = self.active.as_mut()?;
        let result = (&mut job.task).await;
        let job = self.active.take()?;
        if job.session != session {
            crate::logging::info(&format!(
                "Agent instruction generation stale id={} elapsed_ms={}",
                job.id,
                job.started.elapsed().as_millis()
            ));
            return None;
        }
        let result = result.unwrap_or_else(|_| {
            Err(anyhow::anyhow!(
                "Instruction generation interrupted. Retry or write manually."
            ))
        });
        crate::logging::info(&format!(
            "Agent instruction generation finished id={} session={} status={} elapsed_ms={}",
            job.id,
            job.session,
            if result.is_ok() {
                "completed"
            } else {
                "failed"
            },
            job.started.elapsed().as_millis()
        ));
        Some(match result {
            Ok(result) => ServerEvent::AgentInstructionsGenerated {
                id: job.id,
                text: Some(result.text),
                model: result.model,
                provider_name: result.provider_name,
                error: None,
            },
            Err(error) => ServerEvent::AgentInstructionsGenerated {
                id: job.id,
                text: None,
                model: job.model,
                provider_name: job.provider_name,
                error: Some(error.to_string()),
            },
        })
    }
}

impl Drop for InstructionJob {
    fn drop(&mut self) {
        self.clear();
    }
}

pub(super) fn event_log_description(event: &ServerEvent) -> String {
    match event {
        ServerEvent::AgentInstructionsGenerated {
            id, text, error, ..
        } => format!(
            "AgentInstructionsGenerated id={id} bytes={} failed={}",
            text.as_ref().map_or(0, String::len),
            error.is_some()
        ),
        _ => crate::logging::truncate_for_log(&format!("{event:?}"), 200),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::message::{Message, StreamEvent, ToolDefinition};
    use crate::provider::EventStream;
    use async_trait::async_trait;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::time::Duration;

    struct TestProvider {
        pending: bool,
        dropped: Arc<AtomicBool>,
        started: Arc<AtomicBool>,
    }
    struct DropGuard(Arc<AtomicBool>);
    impl Drop for DropGuard {
        fn drop(&mut self) {
            self.0.store(true, Ordering::SeqCst);
        }
    }
    #[async_trait]
    impl Provider for TestProvider {
        async fn complete(
            &self,
            messages: &[Message],
            tools: &[ToolDefinition],
            _system: &str,
            resume: Option<&str>,
        ) -> Result<EventStream> {
            self.started.store(true, Ordering::SeqCst);
            assert_eq!(messages.len(), 1);
            assert!(tools.is_empty());
            assert!(resume.is_none());
            if self.pending {
                let guard = DropGuard(self.dropped.clone());
                Ok(Box::pin(futures::stream::once(async move {
                    let _guard = guard;
                    std::future::pending::<Result<StreamEvent>>().await
                })))
            } else {
                Ok(Box::pin(futures::stream::iter(vec![Ok(
                    StreamEvent::TextDelta("Review carefully.".into()),
                )])))
            }
        }
        fn name(&self) -> &str {
            "fixture"
        }
        fn model(&self) -> String {
            "fixture-model".into()
        }
        fn fork(&self) -> Arc<dyn Provider> {
            Arc::new(Self {
                pending: self.pending,
                dropped: self.dropped.clone(),
                started: self.started.clone(),
            })
        }
        fn fork_for_instruction_generation(&self) -> Result<Arc<dyn Provider>> {
            Ok(self.fork())
        }
    }

    fn start(
        job: &mut InstructionJob,
        id: u64,
        pending: bool,
        dropped: Arc<AtomicBool>,
    ) -> Result<()> {
        job.start(
            id,
            "session-a",
            Arc::new(TestProvider {
                pending,
                dropped,
                started: Arc::default(),
            }),
            "Reviewer".into(),
            "Review changes".into(),
            "all".into(),
        )
    }

    #[tokio::test]
    async fn agent_instructions_job_correlates_result() {
        let mut job = InstructionJob::default();
        assert!(start(&mut job, 41, false, Arc::default()).is_ok());
        let event = job.next("session-a").await.unwrap();
        assert!(
            matches!(event, ServerEvent::AgentInstructionsGenerated { id: 41, text: Some(ref text), error: None, .. } if text == "Review carefully.")
        );
        assert!(!job.is_running());
    }

    #[tokio::test]
    async fn agent_instructions_job_rejects_duplicate_and_correlates_cancel() {
        let mut job = InstructionJob::default();
        assert!(start(&mut job, 41, true, Arc::default()).is_ok());
        assert!(start(&mut job, 42, false, Arc::default()).is_err());
        assert!(!job.cancel(42));
        assert!(job.is_running());
        assert!(job.cancel(41));
        assert!(!job.is_running());
        assert!(start(&mut job, 43, false, Arc::default()).is_ok());
        assert!(matches!(
            job.next("session-a").await,
            Some(ServerEvent::AgentInstructionsGenerated { id: 43, .. })
        ));
    }

    #[tokio::test]
    async fn agent_instructions_job_rejects_stale_session() {
        let mut job = InstructionJob::default();
        assert!(start(&mut job, 41, false, Arc::default()).is_ok());
        assert!(job.next("session-b").await.is_none());
        assert!(!job.is_running());
    }

    #[tokio::test]
    async fn agent_instructions_job_drop_and_session_clear_abort_stream() {
        for disconnect in [false, true] {
            let dropped = Arc::new(AtomicBool::new(false));
            let mut job = InstructionJob::default();
            assert!(start(&mut job, 41, true, dropped.clone()).is_ok());
            tokio::task::yield_now().await;
            if disconnect {
                drop(job);
            } else {
                job.clear();
            }
            tokio::time::timeout(Duration::from_secs(1), async {
                while !dropped.load(Ordering::SeqCst) {
                    tokio::task::yield_now().await;
                }
            })
            .await
            .expect("generation stream must be dropped on disconnect/session replacement");
        }
    }
    use super::super::{
        AwaitMembersRuntime, ClientDebugState, FileTouchService, SessionInterruptQueues,
        SwarmMutationRuntime,
    };
    use crate::agent::Agent;
    use crate::protocol::Request;
    use crate::transport::{ReadHalf, Stream, WriteHalf};
    use std::collections::HashMap;
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
    use tokio::sync::{Mutex, RwLock, broadcast};
    type TestSessions = Arc<RwLock<HashMap<String, Arc<Mutex<Agent>>>>>;

    async fn socket_fixture(
        pending: bool,
        started: Arc<AtomicBool>,
        dropped: Arc<AtomicBool>,
    ) -> (
        BufReader<ReadHalf>,
        WriteHalf,
        tokio::task::JoinHandle<Result<()>>,
        TestSessions,
    ) {
        let (server_stream, client_stream) = Stream::pair().unwrap();
        let provider_template: Arc<dyn Provider> = Arc::new(TestProvider {
            pending,
            dropped,
            started,
        });
        let sessions: TestSessions = Arc::new(RwLock::new(HashMap::new()));
        let global_session_id = Arc::new(RwLock::new(String::new()));
        let client_count = Arc::new(RwLock::new(0usize));
        let client_connections = Arc::new(RwLock::new(HashMap::new()));
        let swarm_members = Arc::new(RwLock::new(HashMap::new()));
        let swarms_by_id = Arc::new(RwLock::new(HashMap::new()));
        let shared_context = Arc::new(RwLock::new(HashMap::new()));
        let swarm_plans = Arc::new(RwLock::new(HashMap::new()));
        let swarm_coordinators = Arc::new(RwLock::new(HashMap::new()));
        let file_touch = FileTouchService::new();
        let channel_subscriptions = Arc::new(RwLock::new(HashMap::new()));
        let channel_subscriptions_by_session = Arc::new(RwLock::new(HashMap::new()));
        let client_debug_state = Arc::new(RwLock::new(ClientDebugState::default()));
        let (_debug_response_tx, _) = broadcast::channel(8);
        let event_history = Arc::new(RwLock::new(std::collections::VecDeque::new()));
        let event_counter = Arc::new(std::sync::atomic::AtomicU64::new(0));
        let (swarm_event_tx, _) = broadcast::channel(8);
        let (_global_event_tx, _) = broadcast::channel(8);
        let global_is_processing = Arc::new(RwLock::new(false));
        let shutdown_signals = Arc::new(RwLock::new(HashMap::new()));
        let soft_interrupt_queues: SessionInterruptQueues = Arc::new(RwLock::new(HashMap::new()));
        let mcp_pool = Arc::new(crate::mcp::SharedMcpPool::from_default_config());

        let server_task = tokio::spawn(super::super::client_lifecycle::handle_client(
            server_stream,
            Arc::clone(&sessions),
            _global_event_tx,
            provider_template,
            global_is_processing,
            global_session_id,
            client_count,
            Arc::clone(&client_connections),
            swarm_members,
            swarms_by_id,
            shared_context,
            swarm_plans,
            swarm_coordinators,
            file_touch,
            channel_subscriptions,
            channel_subscriptions_by_session,
            client_debug_state,
            _debug_response_tx,
            event_history,
            event_counter,
            swarm_event_tx,
            "jcode-test".to_string(),
            "🧪".to_string(),
            mcp_pool,
            shutdown_signals,
            soft_interrupt_queues,
            AwaitMembersRuntime::default(),
            SwarmMutationRuntime::default(),
        ));

        let (reader, writer) = client_stream.into_split();
        (BufReader::new(reader), writer, server_task, sessions)
    }

    async fn send(writer: &mut WriteHalf, request: Request) {
        writer
            .write_all((serde_json::to_string(&request).unwrap() + "\n").as_bytes())
            .await
            .unwrap();
    }

    async fn receive(reader: &mut BufReader<ReadHalf>) -> ServerEvent {
        let mut line = String::new();
        tokio::time::timeout(Duration::from_secs(3), reader.read_line(&mut line))
            .await
            .unwrap()
            .unwrap();
        serde_json::from_str(&line).unwrap()
    }

    struct TestHome {
        old: Option<std::ffi::OsString>,
        root: tempfile::TempDir,
    }
    impl TestHome {
        fn new() -> Self {
            let root = tempfile::tempdir().unwrap();
            let old = std::env::var_os("JCODE_HOME");
            crate::env::set_var("JCODE_HOME", root.path());
            Self { old, root }
        }
    }
    impl Drop for TestHome {
        fn drop(&mut self) {
            if let Some(old) = &self.old {
                crate::env::set_var("JCODE_HOME", old);
            } else {
                crate::env::remove_var("JCODE_HOME");
            }
        }
    }

    async fn subscribe_socket(
        reader: &mut BufReader<ReadHalf>,
        writer: &mut WriteHalf,
        root: &std::path::Path,
    ) {
        send(
            writer,
            Request::Subscribe {
                id: 1,
                working_dir: Some(root.to_string_lossy().into()),
                selfdev: None,
                target_session_id: None,
                client_instance_id: None,
                client_has_local_history: false,
                allow_session_takeover: false,
                crash_on_disconnect: false,
                continue_on_disconnect: false,
                terminal_env: vec![],
            },
        )
        .await;
        loop {
            if matches!(receive(reader).await, ServerEvent::SessionId { .. }) {
                break;
            }
        }
    }

    #[tokio::test]
    #[allow(clippy::await_holding_lock)]
    async fn agent_instructions_socket_returns_draft_without_turn() {
        let _lock = crate::storage::lock_test_env();
        let home = TestHome::new();
        let (mut reader, mut writer, task, sessions) =
            socket_fixture(false, Arc::default(), Arc::default()).await;
        subscribe_socket(&mut reader, &mut writer, home.root.path()).await;
        let agent = sessions.read().await.values().next().unwrap().clone();
        let before = agent.lock().await.messages().len();
        send(
            &mut writer,
            Request::GenerateAgentInstructions {
                id: 41,
                description: "Reviewer".into(),
                purpose: "Review code".into(),
                mode: "all".into(),
            },
        )
        .await;
        let event = loop {
            let event = receive(&mut reader).await;
            if matches!(
                event,
                ServerEvent::AgentInstructionsGenerated { id: 41, .. }
                    | ServerEvent::Error { id: 41, .. }
            ) {
                break event;
            }
        };
        task.abort();
        let _ = task.await;
        assert!(
            matches!(event, ServerEvent::AgentInstructionsGenerated { id: 41, text: Some(ref text), error: None, .. } if text == "Review carefully."),
            "expected draft, got {event:?}"
        );
        assert_eq!(agent.try_lock().unwrap().messages().len(), before);
    }

    async fn wait_flag(flag: &AtomicBool) {
        tokio::time::timeout(Duration::from_secs(1), async {
            while !flag.load(Ordering::SeqCst) {
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("expected provider lifecycle transition");
    }

    #[tokio::test]
    #[allow(clippy::await_holding_lock)]
    async fn agent_instructions_socket_duplicate_cancel_and_mutex_independence() {
        let _lock = crate::storage::lock_test_env();
        let home = TestHome::new();
        let started = Arc::new(AtomicBool::new(false));
        let dropped = Arc::new(AtomicBool::new(false));
        let (mut reader, mut writer, task, sessions) =
            socket_fixture(true, started.clone(), dropped.clone()).await;
        subscribe_socket(&mut reader, &mut writer, home.root.path()).await;
        send(
            &mut writer,
            Request::GenerateAgentInstructions {
                id: 41,
                description: "Reviewer".into(),
                purpose: "Review code".into(),
                mode: "all".into(),
            },
        )
        .await;
        wait_flag(&started).await;
        let agent = sessions.read().await.values().next().unwrap().clone();
        assert!(
            agent.try_lock().is_ok(),
            "provider stream must not hold Agent mutex"
        );
        let message_count = agent.try_lock().unwrap().messages().len();
        send(
            &mut writer,
            Request::GenerateAgentInstructions {
                id: 42,
                description: "Duplicate".into(),
                purpose: "Duplicate".into(),
                mode: "all".into(),
            },
        )
        .await;
        loop {
            if let ServerEvent::AgentInstructionsGenerated { id: 42, error, .. } =
                receive(&mut reader).await
            {
                assert!(error.unwrap().contains("already running"));
                break;
            }
        }
        send(
            &mut writer,
            Request::CancelAgentInstructions {
                id: 43,
                generation_id: 42,
            },
        )
        .await;
        send(&mut writer, Request::Ping { id: 44 }).await;
        loop {
            if matches!(receive(&mut reader).await, ServerEvent::Pong { id: 44, .. }) {
                break;
            }
        }
        assert!(
            !dropped.load(Ordering::SeqCst),
            "mismatched cancel must leave active generation alive"
        );
        send(
            &mut writer,
            Request::CancelAgentInstructions {
                id: 45,
                generation_id: 41,
            },
        )
        .await;
        let cancelled = tokio::time::timeout(Duration::from_millis(250), async {
            while !dropped.load(Ordering::SeqCst) {
                tokio::task::yield_now().await;
            }
        })
        .await
        .is_ok();
        task.abort();
        let _ = task.await;
        assert!(
            cancelled,
            "matching wire cancellation must drop provider stream"
        );
        assert_eq!(agent.try_lock().unwrap().messages().len(), message_count);
    }

    #[tokio::test]
    #[allow(clippy::await_holding_lock)]
    async fn agent_instructions_socket_disconnect_and_clear_cancel_generation() {
        let _lock = crate::storage::lock_test_env();
        let home = TestHome::new();
        for disconnect in [true, false] {
            let started = Arc::new(AtomicBool::new(false));
            let dropped = Arc::new(AtomicBool::new(false));
            let (mut reader, mut writer, task, _) =
                socket_fixture(true, started.clone(), dropped.clone()).await;
            subscribe_socket(&mut reader, &mut writer, home.root.path()).await;
            send(
                &mut writer,
                Request::GenerateAgentInstructions {
                    id: 41,
                    description: "Reviewer".into(),
                    purpose: "Review code".into(),
                    mode: "all".into(),
                },
            )
            .await;
            wait_flag(&started).await;
            if disconnect {
                drop(writer);
                drop(reader);
            } else {
                send(&mut writer, Request::Clear { id: 42 }).await;
            }
            let cancelled = tokio::time::timeout(Duration::from_millis(250), async {
                while !dropped.load(Ordering::SeqCst) {
                    tokio::task::yield_now().await;
                }
            })
            .await
            .is_ok();
            task.abort();
            let _ = task.await;
            assert!(
                cancelled,
                "disconnect={disconnect} must cancel generation promptly"
            );
        }
    }

    #[tokio::test]
    #[allow(clippy::await_holding_lock)]
    async fn agent_instructions_socket_resume_and_subscribe_cancel_generation() {
        let _lock = crate::storage::lock_test_env();
        let home = TestHome::new();
        for subscribe in [false, true] {
            let started = Arc::new(AtomicBool::new(false));
            let dropped = Arc::new(AtomicBool::new(false));
            let (mut reader, mut writer, task, sessions) =
                socket_fixture(true, started.clone(), dropped.clone()).await;
            subscribe_socket(&mut reader, &mut writer, home.root.path()).await;
            let provider: Arc<dyn Provider> = Arc::new(TestProvider {
                pending: false,
                started: Arc::default(),
                dropped: Arc::default(),
            });
            let registry = crate::tool::Registry::new(provider.clone()).await;
            let mut session = crate::session::Session::create_with_id(
                "session_instruction_target".into(),
                None,
                None,
            );
            session.working_dir = Some(home.root.path().to_string_lossy().into_owned());
            sessions.write().await.insert(
                session.id.clone(),
                Arc::new(Mutex::new(Agent::new_with_session(
                    provider, registry, session, None,
                ))),
            );
            send(
                &mut writer,
                Request::GenerateAgentInstructions {
                    id: 41,
                    description: "Reviewer".into(),
                    purpose: "Review".into(),
                    mode: "all".into(),
                },
            )
            .await;
            wait_flag(&started).await;
            let request = if subscribe {
                Request::Subscribe {
                    id: 42,
                    working_dir: None,
                    selfdev: None,
                    target_session_id: Some("session_instruction_target".into()),
                    client_instance_id: None,
                    client_has_local_history: false,
                    allow_session_takeover: false,
                    crash_on_disconnect: false,
                    continue_on_disconnect: false,
                    terminal_env: vec![],
                }
            } else {
                Request::ResumeSession {
                    id: 42,
                    session_id: "session_instruction_target".into(),
                    client_instance_id: None,
                    client_has_local_history: false,
                    allow_session_takeover: false,
                }
            };
            send(&mut writer, request).await;
            wait_flag(&dropped).await;
            send(&mut writer, Request::Ping { id: 43 }).await;
            loop {
                let event = receive(&mut reader).await;
                assert!(
                    !matches!(
                        event,
                        ServerEvent::AgentInstructionsGenerated { id: 41, .. }
                    ),
                    "cancelled session must never publish a draft"
                );
                if matches!(event, ServerEvent::Pong { id: 43, .. }) {
                    break;
                }
            }
            task.abort();
            let _ = task.await;
        }
    }

    #[test]
    fn agent_instructions_failed_write_log_omits_generated_body() {
        let event = ServerEvent::AgentInstructionsGenerated {
            id: 41,
            text: Some("GENERATED_BODY_SECRET".into()),
            model: "model".into(),
            provider_name: "provider".into(),
            error: None,
        };
        let description = event_log_description(&event);
        assert!(description.contains("41"));
        assert!(!description.contains("GENERATED_BODY_SECRET"));
    }
}
