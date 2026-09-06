use super::*;

use std::sync::atomic::{AtomicBool, Ordering};

struct TierProvider {
    name: &'static str,
    tier: std::sync::Mutex<Option<String>>,
}

impl TierProvider {
    fn new(name: &'static str, tier: Option<&str>) -> Self {
        Self {
            name,
            tier: std::sync::Mutex::new(tier.map(str::to_string)),
        }
    }
}

#[async_trait]
impl Provider for TierProvider {
    async fn complete(
        &self,
        _: &[Message],
        _: &[ToolDefinition],
        _: &str,
        _: Option<&str>,
    ) -> Result<EventStream> {
        anyhow::bail!("worker policy test must not make a model request")
    }
    fn name(&self) -> &str {
        self.name
    }
    fn model(&self) -> String {
        "gpt-5.6-luna".to_string()
    }
    fn set_model(&self, _: &str) -> Result<()> {
        Ok(())
    }
    fn service_tier(&self) -> Option<String> {
        self.tier.lock().unwrap().clone()
    }
    fn set_service_tier(&self, tier: &str) -> Result<()> {
        assert_eq!(
            self.name, "OpenAI",
            "must not apply an OpenAI tier to another provider"
        );
        *self.tier.lock().unwrap() = match tier {
            "off" => None,
            "priority" | "flex" => Some(tier.to_string()),
            _ => anyhow::bail!("invalid test tier"),
        };
        Ok(())
    }
    fn fork(&self) -> Arc<dyn Provider> {
        Arc::new(Self::new(self.name, self.service_tier().as_deref()))
    }
}

struct PolicyEnv {
    _home: tempfile::TempDir,
    previous: Vec<(&'static str, Option<std::ffi::OsString>)>,
}
impl PolicyEnv {
    fn new(worker: Option<&str>, main: &str) -> Self {
        let home = tempfile::tempdir().unwrap();
        let previous = [
            "JCODE_HOME",
            "JCODE_NO_TELEMETRY",
            "JCODE_SWARM_OPENAI_SERVICE_TIER",
            "JCODE_OPENAI_SERVICE_TIER",
        ]
        .into_iter()
        .map(|k| (k, std::env::var_os(k)))
        .collect();
        crate::env::set_var("JCODE_HOME", home.path());
        crate::env::set_var("JCODE_NO_TELEMETRY", "1");
        crate::env::set_var("JCODE_OPENAI_SERVICE_TIER", main);
        match worker {
            Some(value) => crate::env::set_var("JCODE_SWARM_OPENAI_SERVICE_TIER", value),
            None => crate::env::remove_var("JCODE_SWARM_OPENAI_SERVICE_TIER"),
        }
        crate::config::Config::invalidate_cache();
        Self {
            _home: home,
            previous,
        }
    }
}
impl Drop for PolicyEnv {
    fn drop(&mut self) {
        for (key, value) in self.previous.drain(..) {
            match value {
                Some(value) => crate::env::set_var(key, value),
                None => crate::env::remove_var(key),
            }
        }
        crate::config::Config::invalidate_cache();
    }
}

fn agent(provider: Arc<dyn Provider>, parent: Option<String>) -> Agent {
    Agent::new_with_session(
        provider,
        Registry::empty(),
        Session::create(parent, None),
        Some(HashSet::new()),
    )
}

#[test]
fn worker_openai_tier_is_independent_in_both_directions() {
    let _lock = crate::storage::lock_test_env();
    for (main, worker, expected) in [
        (None, "priority", Some("priority")),
        (Some("priority"), "off", None),
    ] {
        let _env = PolicyEnv::new(Some(worker), main.unwrap_or("off"));
        let parent: Arc<dyn Provider> = Arc::new(TierProvider::new("OpenAI", main));
        let worker_provider = parent.fork();
        let mut child = agent(worker_provider.clone(), Some("parent".into()));
        child.initialize_spawn_openai_service_tier().unwrap();
        assert_eq!(parent.service_tier().as_deref(), main);
        assert_eq!(worker_provider.service_tier().as_deref(), expected);
        assert_eq!(
            child.session.spawn_openai_service_tier.as_deref(),
            Some(worker)
        );
        worker_provider.set_service_tier("flex").unwrap();
        assert_eq!(
            parent.service_tier().as_deref(),
            main,
            "session override leaked to parent"
        );
    }
}

#[test]
fn worker_openai_tier_unset_or_inherit_preserves_existing_behavior() {
    let _lock = crate::storage::lock_test_env();
    for setting in [None, Some(""), Some(" inherit ")] {
        let _env = PolicyEnv::new(setting, "off");
        let provider: Arc<dyn Provider> = Arc::new(TierProvider::new("OpenAI", Some("flex")));
        let mut child = agent(provider.clone(), Some("parent".into()));
        child.initialize_spawn_openai_service_tier().unwrap();
        assert_eq!(provider.service_tier().as_deref(), Some("flex"));
        assert!(child.session.spawn_openai_service_tier.is_none());
    }
}

#[test]
fn worker_openai_tier_ignores_other_providers_and_ordinary_parented_sessions() {
    let _lock = crate::storage::lock_test_env();
    let _env = PolicyEnv::new(Some("priority"), "off");
    let other: Arc<dyn Provider> = Arc::new(TierProvider::new("Anthropic", None));
    let mut child = agent(other.clone(), Some("parent".into()));
    child.initialize_spawn_openai_service_tier().unwrap();
    assert!(other.service_tier().is_none());
    let normal_provider: Arc<dyn Provider> = Arc::new(TierProvider::new("OpenAI", None));
    let normal = agent(
        normal_provider.clone(),
        Some("ordinary-split-parent".into()),
    );
    assert!(normal.session.spawn_openai_service_tier.is_none());
    assert!(normal_provider.service_tier().is_none());
}

#[test]
fn worker_openai_tier_normalizes_aliases_and_rejects_invalid_policy() {
    let _lock = crate::storage::lock_test_env();
    for (raw, expected) in [
        (" FAST ", "priority"),
        ("standard", "off"),
        ("flex", "flex"),
    ] {
        let _env = PolicyEnv::new(Some(raw), "off");
        assert_eq!(
            Agent::configured_spawn_openai_service_tier()
                .unwrap()
                .as_deref(),
            Some(expected)
        );
    }
    let _env = PolicyEnv::new(Some("prority"), "off");
    let error = Agent::configured_spawn_openai_service_tier().unwrap_err();
    assert!(
        error
            .to_string()
            .contains("agents.swarm_openai_service_tier")
    );
}

#[test]
fn worker_openai_tier_survives_resume_without_leaking_into_main_session() {
    let _lock = crate::storage::lock_test_env();
    let _env = PolicyEnv::new(Some("priority"), "off");
    let mut worker_session = Session::create(Some("parent".into()), None);
    worker_session.spawn_openai_service_tier = Some("priority".into());
    worker_session.save().unwrap();
    assert_eq!(
        Session::load(&worker_session.id)
            .unwrap()
            .spawn_openai_service_tier
            .as_deref(),
        Some("priority"),
        "a marker-only worker session must be persisted"
    );
    let mut main_session = Session::create(None, None);
    main_session.title = Some("main restore fixture".into());
    main_session.save().unwrap();
    let provider: Arc<dyn Provider> = Arc::new(TierProvider::new("OpenAI", None));
    let mut attached = Agent::new_with_session(
        provider.clone(),
        Registry::empty(),
        worker_session,
        Some(HashSet::new()),
    );
    assert_eq!(provider.service_tier().as_deref(), Some("priority"));
    attached.restore_session(&main_session.id).unwrap();
    assert!(
        provider.service_tier().is_none(),
        "worker tier leaked into main attach"
    );
}

struct SwitchableTierProvider {
    openai: AtomicBool,
    model: std::sync::Mutex<String>,
    tier: std::sync::Mutex<Option<String>>,
    fail_priority: AtomicBool,
}

impl SwitchableTierProvider {
    fn new(openai: bool, model: &str, tier: Option<&str>) -> Self {
        Self {
            openai: AtomicBool::new(openai),
            model: std::sync::Mutex::new(model.to_string()),
            tier: std::sync::Mutex::new(tier.map(str::to_string)),
            fail_priority: AtomicBool::new(false),
        }
    }

    fn fail_priority_restores(&self) {
        self.fail_priority.store(true, Ordering::SeqCst);
    }
}

#[async_trait]
impl Provider for SwitchableTierProvider {
    async fn complete(
        &self,
        _: &[Message],
        _: &[ToolDefinition],
        _: &str,
        _: Option<&str>,
    ) -> Result<EventStream> {
        anyhow::bail!("worker policy test must not make a model request")
    }

    fn name(&self) -> &str {
        if self.openai.load(Ordering::SeqCst) {
            "OpenAI"
        } else {
            "Anthropic"
        }
    }

    fn model(&self) -> String {
        self.model.lock().unwrap().clone()
    }

    fn set_model(&self, model: &str) -> Result<()> {
        self.openai
            .store(model.to_ascii_lowercase().contains("gpt"), Ordering::SeqCst);
        *self.model.lock().unwrap() = model.to_string();
        Ok(())
    }

    fn service_tier(&self) -> Option<String> {
        self.tier.lock().unwrap().clone()
    }

    fn set_service_tier(&self, tier: &str) -> Result<()> {
        assert_eq!(
            self.name(),
            "OpenAI",
            "must not apply an OpenAI tier to another provider"
        );
        if tier == "priority" && self.fail_priority.swap(false, Ordering::SeqCst) {
            anyhow::bail!("simulated worker tier restore failure")
        }
        *self.tier.lock().unwrap() = match tier {
            "off" => None,
            "priority" | "flex" => Some(tier.to_string()),
            _ => anyhow::bail!("invalid test tier"),
        };
        Ok(())
    }

    fn fork(&self) -> Arc<dyn Provider> {
        Arc::new(Self::new(
            self.openai.load(Ordering::SeqCst),
            &self.model(),
            self.service_tier().as_deref(),
        ))
    }
}

#[test]
fn worker_openai_tier_reapplies_after_switching_openai_other_openai() {
    let _lock = crate::storage::lock_test_env();
    let _env = PolicyEnv::new(Some("priority"), "off");
    let provider = Arc::new(SwitchableTierProvider::new(true, "gpt-5.6-luna", None));
    let mut worker = agent(provider.clone(), Some("parent".into()));
    worker.initialize_spawn_openai_service_tier().unwrap();
    assert_eq!(provider.service_tier().as_deref(), Some("priority"));

    worker.set_model("claude-sonnet").unwrap();
    assert_eq!(provider.name(), "Anthropic");
    assert!(provider.service_tier().is_none());

    worker.set_model("gpt-5.6-luna").unwrap();
    assert_eq!(provider.name(), "OpenAI");
    assert_eq!(provider.service_tier().as_deref(), Some("priority"));
}

#[test]
fn worker_openai_tier_applies_when_switching_other_to_openai() {
    let _lock = crate::storage::lock_test_env();
    let _env = PolicyEnv::new(Some("priority"), "off");
    let provider = Arc::new(SwitchableTierProvider::new(false, "claude-sonnet", None));
    let mut worker = agent(provider.clone(), Some("parent".into()));
    worker.session.spawn_openai_service_tier = Some("priority".into());

    worker.set_model("gpt-5.6-luna").unwrap();
    assert_eq!(provider.name(), "OpenAI");
    assert_eq!(provider.service_tier().as_deref(), Some("priority"));
}

#[test]
fn worker_openai_tier_restoring_non_openai_main_clears_marker_and_prevents_releak() {
    let _lock = crate::storage::lock_test_env();
    let _env = PolicyEnv::new(Some("priority"), "off");
    let mut worker_session = Session::create(Some("parent".into()), None);
    worker_session.model = Some("gpt-5.6-luna".into());
    worker_session.spawn_openai_service_tier = Some("priority".into());
    worker_session.save().unwrap();
    let mut main_session = Session::create(None, None);
    main_session.title = Some("non-openai main restore fixture".into());
    main_session.model = Some("claude-sonnet".into());
    main_session.save().unwrap();

    let provider = Arc::new(SwitchableTierProvider::new(true, "gpt-5.6-luna", None));
    let mut worker = Agent::new_with_session(
        provider.clone(),
        Registry::empty(),
        worker_session,
        Some(HashSet::new()),
    );
    assert_eq!(provider.service_tier().as_deref(), Some("priority"));

    worker.restore_session(&main_session.id).unwrap();
    assert_eq!(provider.name(), "Anthropic");
    assert!(provider.service_tier().is_none());
    assert!(worker.session.spawn_openai_service_tier.is_none());

    worker.set_model("gpt-5.6-luna").unwrap();
    assert_eq!(provider.name(), "OpenAI");
    assert!(provider.service_tier().is_none());
}

#[test]
fn worker_openai_tier_clearing_session_resets_to_main_default() {
    let _lock = crate::storage::lock_test_env();
    let _env = PolicyEnv::new(Some("priority"), "off");
    let provider = Arc::new(SwitchableTierProvider::new(true, "gpt-5.6-luna", None));
    let mut worker = agent(provider.clone(), Some("parent".into()));
    worker.session.spawn_openai_service_tier = Some("priority".into());
    worker.restore_spawn_openai_service_tier().unwrap();
    assert_eq!(provider.service_tier().as_deref(), Some("priority"));

    worker.clear();
    assert!(worker.session.spawn_openai_service_tier.is_none());
    assert!(provider.service_tier().is_none());
}

#[test]
fn worker_openai_tier_failed_restore_does_not_abort_session_lifecycle() {
    let _lock = crate::storage::lock_test_env();
    let _env = PolicyEnv::new(Some("priority"), "off");
    let mut worker_session = Session::create(Some("parent".into()), None);
    worker_session.model = Some("gpt-5.6-luna".into());
    worker_session.spawn_openai_service_tier = Some("priority".into());
    worker_session.save().unwrap();
    let mut main_session = Session::create(None, None);
    main_session.title = Some("failed restore main fixture".into());
    let provider = Arc::new(SwitchableTierProvider::new(true, "gpt-5.6-luna", None));
    provider.fail_priority_restores();
    let mut agent = Agent::new_with_session(
        provider.clone(),
        Registry::empty(),
        main_session,
        Some(HashSet::new()),
    );

    agent.restore_session(&worker_session.id).unwrap();
    assert_eq!(agent.session.id, worker_session.id);
    assert_eq!(agent.session.status, crate::session::SessionStatus::Active);
    assert_eq!(
        agent
            .concurrency_session
            .as_ref()
            .expect("restored session should own concurrency tracking")
            .session_id(),
        worker_session.id
    );
    assert_eq!(provider.name(), "OpenAI");
    assert!(provider.service_tier().is_none());
}

#[test]
fn worker_openai_tier_provider_lookup_tracks_clear_restore_drop_and_replacement() {
    let _lock = crate::storage::lock_test_env();
    let _env = PolicyEnv::new(None, "off");
    let first: Arc<dyn Provider> = Arc::new(TierProvider::new("OpenAI", None));
    let mut worker = agent(first.clone(), None);
    let original_id = worker.session_id().to_string();
    assert!(Arc::ptr_eq(
        &Agent::provider_handle_for_session(&original_id).unwrap(),
        &first
    ));
    worker.clear();
    assert!(Agent::provider_handle_for_session(&original_id).is_none());
    let cleared_id = worker.session_id().to_string();
    let mut restored = Session::create(None, Some("restore target".into()));
    restored.save().unwrap();
    worker.restore_session(&restored.id).unwrap();
    assert!(Agent::provider_handle_for_session(&cleared_id).is_none());
    assert!(Arc::ptr_eq(
        &Agent::provider_handle_for_session(&restored.id).unwrap(),
        &first
    ));

    let replacement_provider: Arc<dyn Provider> =
        Arc::new(TierProvider::new("OpenAI", Some("flex")));
    let replacement = Agent::new_with_session(
        replacement_provider.clone(),
        Registry::empty(),
        restored.clone(),
        Some(HashSet::new()),
    );
    drop(worker);
    assert!(
        Arc::ptr_eq(
            &Agent::provider_handle_for_session(&restored.id).unwrap(),
            &replacement_provider
        ),
        "old registration removed its replacement"
    );
    drop(replacement);
    assert!(
        Agent::provider_handle_for_session(&restored.id).is_none(),
        "registration must be removed even when provider has other owners"
    );
}
