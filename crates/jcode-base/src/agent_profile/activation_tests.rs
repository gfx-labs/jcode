use super::*;
use crate::message::{Message, ToolDefinition};
use crate::provider::{EventStream, Provider};
use std::sync::Mutex;

struct TestHome(Option<std::ffi::OsString>);
impl TestHome {
    fn set(path: &Path) -> Self {
        let old = std::env::var_os("JCODE_HOME");
        crate::env::set_var("JCODE_HOME", path);
        Self(old)
    }
}
impl Drop for TestHome {
    fn drop(&mut self) {
        if let Some(old) = &self.0 {
            crate::env::set_var("JCODE_HOME", old);
        } else {
            crate::env::remove_var("JCODE_HOME");
        }
    }
}

struct TestProvider(Mutex<String>);
#[async_trait::async_trait]
impl Provider for TestProvider {
    async fn complete(
        &self,
        _: &[Message],
        _: &[ToolDefinition],
        _: &str,
        _: Option<&str>,
    ) -> Result<EventStream> {
        bail!("activation must not call the model")
    }
    fn name(&self) -> &str {
        "OpenAI"
    }
    fn model(&self) -> String {
        self.0.lock().unwrap().clone()
    }
    fn set_model(&self, model: &str) -> Result<()> {
        ensure!(model != "invalid", "Unknown model");
        *self.0.lock().unwrap() = model.to_string();
        Ok(())
    }
    fn set_reasoning_effort(&self, _: &str) -> Result<()> {
        bail!("effort unsupported")
    }
    fn fork(&self) -> Arc<dyn Provider> {
        Arc::new(Self(Mutex::new(self.model())))
    }
}

#[test]
fn agent_profile_activation_is_atomic_and_persists_across_reload() {
    let _lock = crate::storage::lock_test_env();
    let root = tempfile::tempdir().unwrap();
    let _home = TestHome::set(root.path());
    let project = tempfile::tempdir().unwrap();
    let dir = project.path().join(".jcode/agents");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        dir.join("reviewer.md"),
        "---\ndescription: Review\nmodel: next\n---\nCheck every change.",
    )
    .unwrap();
    std::fs::write(
        dir.join("bad.md"),
        "---\ndescription: Bad\nmodel: invalid\n---\nNever applied.",
    )
    .unwrap();
    std::fs::write(
        dir.join("effort.md"),
        "---\ndescription: Bad effort\nmodel: another\neffort: high\n---\nNever applied.",
    )
    .unwrap();
    let mut session = crate::session::Session::create(None, None);
    session.working_dir = Some(project.path().display().to_string());
    let mut provider: Arc<dyn Provider> = Arc::new(TestProvider(Mutex::new("original".into())));
    let original = provider.clone();
    let skills = crate::skill::SkillRegistry::default();
    activate_profile(
        &mut session,
        &mut provider,
        &skills,
        Some("reviewer"),
        false,
    )
    .unwrap();
    assert_eq!(provider.model(), "next");
    assert_eq!(original.model(), "original");
    let active = session.agent_profile.clone();
    for name in ["bad", "effort", "missing", "../reviewer"] {
        assert!(activate_profile(&mut session, &mut provider, &skills, Some(name), false).is_err());
        assert_eq!(provider.model(), "next");
        assert_eq!(session.agent_profile, active);
    }
    std::fs::remove_file(dir.join("reviewer.md")).unwrap();
    for loaded in [
        crate::session::Session::load(&session.id).unwrap(),
        crate::session::Session::load_startup_stub(&session.id).unwrap(),
        crate::session::Session::load_for_remote_startup(&session.id).unwrap(),
    ] {
        assert_eq!(loaded.agent_profile, active);
    }
    activate_profile(&mut session, &mut provider, &skills, None, false).unwrap();
    assert_eq!(provider.model(), "next");
    assert!(session.agent_profile.is_none());
    assert!(
        crate::session::Session::load(&session.id)
            .unwrap()
            .agent_profile
            .is_none()
    );
}

#[test]
fn agent_profile_resolves_skill_content_and_reloads_edited_files() {
    let _lock = crate::storage::lock_test_env();
    let root = tempfile::tempdir().unwrap();
    let _home = TestHome::set(root.path());
    let project = tempfile::tempdir().unwrap();
    let dir = project.path().join(".jcode/agents");
    let skill = project.path().join(".jcode/skills/check");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::create_dir_all(&skill).unwrap();
    std::fs::write(
        skill.join("SKILL.md"),
        "---\nname: check\ndescription: Checks\n---\nSkill instruction marker.",
    )
    .unwrap();
    let text = "---\ndescription: Review\nskills: [check]\n---\nProfile instruction marker.";
    std::fs::write(dir.join("reviewer.md"), text).unwrap();
    let registry = AgentProfileRegistry::load(Some(project.path()));
    let skills = crate::skill::SkillRegistry::load_project_overlay(Some(project.path())).unwrap();
    let profile = registry
        .get("reviewer")
        .unwrap()
        .resolve(&skills, false)
        .unwrap();
    assert!(profile.prompt.contains("Skill instruction marker."));
    assert!(profile.prompt.contains("Profile instruction marker."));
    std::fs::write(
        dir.join("reviewer.md"),
        text.replace("Profile instruction marker.", "Edited instructions."),
    )
    .unwrap();
    assert!(
        AgentProfileRegistry::load(Some(project.path()))
            .get("reviewer")
            .unwrap()
            .prompt
            .contains("Edited instructions.")
    );
}

#[test]
fn agent_profile_inherit_and_clear_preserve_saved_route() {
    let _lock = crate::storage::lock_test_env();
    let root = tempfile::tempdir().unwrap();
    let _home = TestHome::set(root.path());
    std::fs::create_dir(root.path().join("agents")).unwrap();
    std::fs::write(
        root.path().join("agents/reviewer.md"),
        "---\ndescription: Review\nmodel: inherit\n---\nCheck changes.",
    )
    .unwrap();
    let mut session = crate::session::Session::create(None, None);
    session.model = Some("openai-api:original".into());
    session.provider_key = Some("openai".into());
    session.route_api_method = Some("responses".into());
    let mut provider: Arc<dyn Provider> = Arc::new(TestProvider(Mutex::new("original".into())));
    for name in [Some("reviewer"), None] {
        activate_profile(
            &mut session,
            &mut provider,
            &crate::skill::SkillRegistry::default(),
            name,
            false,
        )
        .unwrap();
        assert_eq!(session.model.as_deref(), Some("openai-api:original"));
        assert_eq!(session.provider_key.as_deref(), Some("openai"));
        assert_eq!(session.route_api_method.as_deref(), Some("responses"));
        assert_eq!(provider.model(), "original");
    }
    session.provider_key = Some("claude-api".into());
    session.route_api_method = Some("claude-api".into());
    activate_profile(
        &mut session,
        &mut provider,
        &crate::skill::SkillRegistry::default(),
        Some("reviewer"),
        false,
    )
    .unwrap();
    assert_eq!(session.provider_key.as_deref(), Some("openai"));
    assert!(session.route_api_method.is_none());
}
