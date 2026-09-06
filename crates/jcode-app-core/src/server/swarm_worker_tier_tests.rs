use super::*;
use crate::message::{Message, StreamEvent, ToolDefinition};
use crate::provider::{EventStream, Provider};
use crate::tool::Registry;
use async_trait::async_trait;

struct TaskProvider {
    tier: StdMutex<Option<String>>,
    requested_tiers: Arc<StdMutex<Vec<Option<String>>>>,
}

#[async_trait]
impl Provider for TaskProvider {
    async fn complete(
        &self,
        _: &[Message],
        _: &[ToolDefinition],
        _: &str,
        _: Option<&str>,
    ) -> Result<EventStream> {
        self.requested_tiers
            .lock()
            .unwrap()
            .push(self.service_tier());
        Ok(Box::pin(tokio_stream::iter([
            Ok(StreamEvent::TextDelta("task completed".into())),
            Ok(StreamEvent::MessageEnd { stop_reason: None }),
        ])))
    }
    fn name(&self) -> &str {
        "OpenAI"
    }
    fn model(&self) -> String {
        "gpt-5.6-luna".into()
    }
    fn set_model(&self, _: &str) -> Result<()> {
        Ok(())
    }
    fn service_tier(&self) -> Option<String> {
        self.tier.lock().unwrap().clone()
    }
    fn set_service_tier(&self, tier: &str) -> Result<()> {
        *self.tier.lock().unwrap() = (tier != "off").then(|| tier.to_string());
        Ok(())
    }
    fn fork(&self) -> Arc<dyn Provider> {
        Arc::new(Self {
            tier: StdMutex::new(self.service_tier()),
            requested_tiers: self.requested_tiers.clone(),
        })
    }
}

struct RestoreEnv(Vec<(&'static str, Option<std::ffi::OsString>)>);
impl Drop for RestoreEnv {
    fn drop(&mut self) {
        for (key, value) in self.0.drain(..) {
            match value {
                Some(value) => crate::env::set_var(key, value),
                None => crate::env::remove_var(key),
            }
        }
        crate::config::Config::invalidate_cache();
    }
}

#[tokio::test]
async fn short_task_worker_uses_priority_without_mutating_main_tier() -> Result<()> {
    let _lock = crate::storage::lock_test_env();
    let home = tempfile::tempdir()?;
    let _env = RestoreEnv(
        [
            "JCODE_HOME",
            "JCODE_NO_TELEMETRY",
            "JCODE_OPENAI_SERVICE_TIER",
            "JCODE_SWARM_OPENAI_SERVICE_TIER",
        ]
        .into_iter()
        .map(|key| (key, std::env::var_os(key)))
        .collect(),
    );
    crate::env::set_var("JCODE_HOME", home.path());
    crate::env::set_var("JCODE_NO_TELEMETRY", "1");
    crate::env::set_var("JCODE_OPENAI_SERVICE_TIER", "off");
    crate::env::set_var("JCODE_SWARM_OPENAI_SERVICE_TIER", "priority");
    std::fs::write(
        home.path().join("config.toml"),
        "[features]\nmemory = false\n",
    )?;
    crate::config::Config::invalidate_cache();
    let seen = Arc::new(StdMutex::new(Vec::new()));
    let provider: Arc<dyn Provider> = Arc::new(TaskProvider {
        tier: StdMutex::new(None),
        requested_tiers: seen.clone(),
    });
    let parent_session = Session::create(None, Some("task-tier-parent".into()));
    let parent_id = parent_session.id.clone();
    let parent = Arc::new(Mutex::new(Agent::new_with_session(
        provider.clone(),
        Registry::empty(),
        parent_session,
        Some(HashSet::new()),
    )));
    let output = run_swarm_task(parent, "tier check", "general", "complete the task").await?;
    assert!(output.contains("task completed"));
    assert_eq!(*seen.lock().unwrap(), vec![Some("priority".to_string())]);
    assert_eq!(
        provider.service_tier(),
        None,
        "task changed main provider tier"
    );
    let mut workers = Vec::new();
    for entry in std::fs::read_dir(home.path().join("sessions"))? {
        let path = entry?.path();
        if path.extension().is_some_and(|ext| ext == "json") {
            let session: Session = serde_json::from_slice(&std::fs::read(path)?)?;
            if session.parent_id.as_deref() == Some(&parent_id) {
                workers.push(session);
            }
        }
    }
    assert_eq!(workers.len(), 1);
    assert_eq!(
        workers[0].spawn_openai_service_tier.as_deref(),
        Some("priority")
    );
    Ok(())
}
