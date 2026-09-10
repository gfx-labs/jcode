use anyhow::{Context, Result, bail, ensure};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

#[cfg(test)]
mod activation_tests;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AgentMode {
    Primary,
    Subagent,
    #[default]
    All,
}

impl AgentMode {
    pub fn allows_primary(self) -> bool {
        self != Self::Subagent
    }

    pub fn allows_subagent(self) -> bool {
        self != Self::Primary
    }
}

#[derive(Debug, Clone)]
pub struct AgentProfile {
    pub name: String,
    pub description: String,
    pub mode: AgentMode,
    pub model: Option<String>,
    pub effort: Option<String>,
    pub skills: Vec<String>,
    pub prompt: String,
    pub path: PathBuf,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Frontmatter {
    description: String,
    #[serde(default)]
    mode: AgentMode,
    model: Option<String>,
    effort: Option<String>,
    #[serde(default)]
    skills: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ActiveAgentProfile {
    pub name: String,
    pub prompt: String,
}

#[derive(Debug, Default)]
pub struct AgentProfileRegistry {
    pub profiles: BTreeMap<String, AgentProfile>,
    pub errors: Vec<String>,
    invalid: BTreeMap<String, String>,
}

pub fn validate_name(name: &str) -> Result<()> {
    ensure!(
        !name.is_empty()
            && name
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_'),
        "Agent names must contain only letters, digits, hyphens, or underscores"
    );
    Ok(())
}

pub fn parse_profile(path: &Path, content: &str) -> Result<AgentProfile> {
    let name = path
        .file_stem()
        .and_then(|s| s.to_str())
        .context("Invalid agent filename")?;
    validate_name(name)?;
    let content = content.trim_start_matches('\u{feff}').replace("\r\n", "\n");
    let mut lines = content.lines();
    ensure!(
        lines.next() == Some("---"),
        "Agent file must start with YAML frontmatter (---)"
    );
    let mut yaml = String::new();
    let mut closed = false;
    for line in lines.by_ref() {
        if line == "---" {
            closed = true;
            break;
        }
        yaml.push_str(line);
        yaml.push('\n');
    }
    ensure!(closed, "Agent frontmatter is missing its closing ---");
    let mut metadata: Frontmatter = serde_yaml::from_str(&yaml)
        .context("Invalid agent frontmatter (permission and tools fields are not supported)")?;
    ensure!(
        !metadata.description.trim().is_empty(),
        "Agent description must not be empty"
    );
    for (label, value) in [
        ("model", &mut metadata.model),
        ("effort", &mut metadata.effort),
    ] {
        if let Some(value) = value {
            *value = value.trim().to_string();
            ensure!(!value.is_empty(), "Agent {label} must not be empty");
        }
    }
    if let Some(effort) = metadata.effort.as_deref() {
        ensure!(
            matches!(
                effort,
                "none" | "minimal" | "low" | "medium" | "high" | "xhigh" | "max"
            ),
            "Invalid reasoning effort: {effort}"
        );
    }
    for skill in &mut metadata.skills {
        *skill = skill.trim().to_string();
        ensure!(!skill.is_empty(), "Agent skill names must not be empty");
    }
    let prompt = lines.collect::<Vec<_>>().join("\n").trim().to_string();
    ensure!(
        !prompt.is_empty() || !metadata.skills.is_empty(),
        "Agent needs instructions or at least one skill"
    );
    Ok(AgentProfile {
        name: name.to_string(),
        description: metadata.description.trim().to_string(),
        mode: metadata.mode,
        model: metadata.model,
        effort: metadata.effort,
        skills: metadata.skills,
        prompt,
        path: path.to_path_buf(),
    })
}

impl AgentProfileRegistry {
    pub fn load(working_dir: Option<&Path>) -> Self {
        let mut registry = Self::default();
        match crate::storage::jcode_dir() {
            Ok(root) => registry.load_dir(&root.join("agents")),
            Err(error) => registry
                .errors
                .push(format!("Cannot locate global agents: {error}")),
        }
        if let Some(root) = working_dir {
            registry.load_dir(&root.join(".jcode/agents"));
        }
        registry
    }

    fn load_dir(&mut self, directory: &Path) {
        let entries = match std::fs::read_dir(directory) {
            Ok(entries) => entries,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return,
            Err(error) => {
                self.errors
                    .push(format!("{}: {error}", directory.display()));
                return;
            }
        };
        let mut paths = Vec::new();
        for entry in entries {
            match entry {
                Ok(entry) if entry.path().extension().is_some_and(|ext| ext == "md") => {
                    paths.push(entry.path())
                }
                Ok(_) => (),
                Err(error) => self
                    .errors
                    .push(format!("{}: {error}", directory.display())),
            }
        }
        paths.sort();
        for path in paths {
            let name = path
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or_default()
                .to_string();
            // A broken project override must not silently activate the global profile.
            self.profiles.remove(&name);
            self.invalid.remove(&name);
            let result = std::fs::read_to_string(&path)
                .with_context(|| format!("Cannot read {}", path.display()))
                .and_then(|content| parse_profile(&path, &content));
            match result {
                Ok(profile) => {
                    self.profiles.insert(name, profile);
                }
                Err(error) => {
                    let message = format!("{}: {error:#}", path.display());
                    self.invalid.insert(name, message.clone());
                    self.errors.push(message);
                }
            }
        }
    }

    pub fn get(&self, name: &str) -> Result<&AgentProfile> {
        validate_name(name)?;
        if let Some(error) = self.invalid.get(name) {
            bail!("{error}");
        }
        self.profiles.get(name).with_context(|| {
            format!(
                "Unknown agent '{name}'. Add ~/.jcode/agents/{name}.md or .jcode/agents/{name}.md"
            )
        })
    }
}

impl AgentProfile {
    pub fn resolve(
        &self,
        skills: &crate::skill::SkillRegistry,
        for_worker: bool,
    ) -> Result<ActiveAgentProfile> {
        ensure!(
            if for_worker {
                self.mode.allows_subagent()
            } else {
                self.mode.allows_primary()
            },
            "Agent '{}' is {}-only and cannot be used as a {}",
            self.name,
            if self.mode == AgentMode::Subagent {
                "subagent"
            } else {
                "primary"
            },
            if for_worker {
                "worker"
            } else {
                "primary agent"
            }
        );
        let mut prompt = format!("## Custom agent: {}\n\n{}", self.name, self.prompt);
        for name in &self.skills {
            let skill = skills.get(name).with_context(|| {
                format!("Agent '{}' requires missing skill '{name}'", self.name)
            })?;
            prompt.push_str("\n\n");
            prompt.push_str(&skill.get_prompt());
        }
        Ok(ActiveAgentProfile {
            name: self.name.clone(),
            prompt,
        })
    }
}

pub fn activate_profile(
    session: &mut crate::session::Session,
    provider: &mut Arc<dyn crate::provider::Provider>,
    skills: &crate::skill::SkillRegistry,
    name: Option<&str>,
    for_worker: bool,
) -> Result<()> {
    let registry = AgentProfileRegistry::load(session.working_dir.as_deref().map(Path::new));
    let profile = name.map(|name| registry.get(name)).transpose()?;
    let snapshot = profile
        .map(|profile| profile.resolve(skills, for_worker))
        .transpose()?;
    let changes_provider = profile.is_some_and(|profile| {
        profile
            .model
            .as_deref()
            .is_some_and(|model| model != "inherit")
            || profile.effort.is_some()
    });
    let candidate = if changes_provider {
        provider.fork()
    } else {
        Arc::clone(provider)
    };
    if let Some(profile) = profile {
        if let Some(model) = profile.model.as_deref().filter(|m| *m != "inherit") {
            crate::provider::set_model_with_auth_refresh(candidate.as_ref(), model)?;
        }
        if let Some(effort) = profile.effort.as_deref() {
            candidate.set_reasoning_effort(effort)?;
        }
    }
    // Save the candidate before swapping live state, so failed activation is atomic.
    let mut updated = session.clone();
    updated.agent_profile = snapshot;
    if updated.model.is_none() {
        let runtime_model = candidate.model();
        updated.model = Some(
            candidate
                .explicit_provider_pin_for_current_model()
                .map(|pin| format!("{runtime_model}@{pin}"))
                .unwrap_or(runtime_model),
        );
    }
    updated.reasoning_effort = candidate.reasoning_effort();
    let inherited_provider_key =
        crate::provider::MultiProvider::session_provider_key_after_model_switch(
            &candidate.model(),
            candidate.name(),
            session.provider_key.as_deref(),
        );
    if updated.provider_key != inherited_provider_key {
        updated.provider_key = inherited_provider_key;
        updated.route_api_method = None;
    }
    if let Some(model) = profile
        .and_then(|p| p.model.as_deref())
        .filter(|m| *m != "inherit")
    {
        updated.provider_key =
            crate::provider::MultiProvider::session_provider_key_after_model_switch(
                model,
                candidate.name(),
                session.provider_key.as_deref(),
            );
        updated.route_api_method = None;
        updated.model = Some(model.to_string());
    }
    updated.provider_session_id = None;
    updated.save()?;
    *session = updated;
    *provider = candidate;
    crate::logging::info(&format!(
        "Agent profile changed: session={} profile={}",
        session.id,
        name.unwrap_or("default")
    ));
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    const PROFILE: &str = "---\ndescription: Review changes\nmodel: inherit\nmode: all\nskills: [review-qa]\n---\nReview correctness.";

    #[test]
    fn agent_profile_accepts_markdown_frontmatter() {
        let profile = parse_profile(Path::new("reviewer.md"), PROFILE).unwrap();
        assert_eq!(profile.name, "reviewer");
        assert_eq!(profile.model.as_deref(), Some("inherit"));
        assert_eq!(profile.skills, ["review-qa"]);
        assert_eq!(profile.prompt, "Review correctness.");
        assert!(profile.mode.allows_primary() && profile.mode.allows_subagent());
    }

    #[test]
    fn agent_profile_rejects_unsupported_permissions_and_unknown_fields() {
        for field in [
            "permission: {edit: deny}",
            "tools: {bash: false}",
            "modle: typo",
            "mode: other",
            "effort: extreme",
            "skills: ['']",
        ] {
            let text = PROFILE.replace("model: inherit", field);
            assert!(
                parse_profile(Path::new("reviewer.md"), &text).is_err(),
                "{field}"
            );
        }
    }

    #[test]
    fn agent_profile_requires_valid_name_and_content() {
        for path in ["..md", "bad name.md", "bad.name.md"] {
            assert!(parse_profile(Path::new(path), PROFILE).is_err());
        }
        for content in [
            "hello",
            "---\ndescription: test",
            "---\ndescription: ''\n---\nHello",
            "---\ndescription: test\n---\n",
        ] {
            assert!(parse_profile(Path::new("test.md"), content).is_err());
        }
        assert!(validate_name("../reviewer").is_err());
    }

    #[test]
    fn agent_profile_supports_crlf_and_default_mode() {
        let profile = parse_profile(
            Path::new("Reviewer.md"),
            &PROFILE.replace("mode: all\n", "").replace('\n', "\r\n"),
        )
        .unwrap();
        assert_eq!(profile.mode, AgentMode::All);
        assert_eq!(profile.prompt, "Review correctness.");
    }

    #[test]
    fn agent_profile_project_overrides_and_invalid_override_does_not_fall_back() {
        let global = tempfile::tempdir().unwrap();
        let project = tempfile::tempdir().unwrap();
        std::fs::write(global.path().join("reviewer.md"), PROFILE).unwrap();
        std::fs::write(
            project.path().join("reviewer.md"),
            PROFILE.replace("Review correctness.", "Project instructions."),
        )
        .unwrap();
        let mut registry = AgentProfileRegistry::default();
        registry.load_dir(global.path());
        registry.load_dir(project.path());
        assert_eq!(
            registry.get("reviewer").unwrap().prompt,
            "Project instructions."
        );
        std::fs::write(project.path().join("reviewer.md"), "invalid").unwrap();
        registry.load_dir(project.path());
        assert!(registry.get("reviewer").is_err());
        assert_eq!(registry.errors.len(), 1);
    }

    #[test]
    fn agent_profile_discovery_is_sorted_and_ignores_other_files() {
        let root = tempfile::tempdir().unwrap();
        for name in ["z.md", "a.md", "ignore.txt"] {
            std::fs::write(root.path().join(name), PROFILE).unwrap();
        }
        let mut registry = AgentProfileRegistry::default();
        registry.load_dir(&root.path().join("absent"));
        registry.load_dir(root.path());
        assert!(registry.errors.is_empty());
        assert_eq!(
            registry
                .profiles
                .keys()
                .map(String::as_str)
                .collect::<Vec<_>>(),
            ["a", "z"]
        );
    }

    #[test]
    fn agent_profile_missing_skills_and_mode_mismatches_are_errors() {
        let mut profile = parse_profile(Path::new("reviewer.md"), PROFILE).unwrap();
        assert!(
            profile
                .resolve(&crate::skill::SkillRegistry::default(), false)
                .is_err()
        );
        profile.skills.clear();
        profile.mode = AgentMode::Subagent;
        assert!(
            profile
                .resolve(&crate::skill::SkillRegistry::default(), false)
                .is_err()
        );
        assert!(
            profile
                .resolve(&crate::skill::SkillRegistry::default(), true)
                .is_ok()
        );
        profile.mode = AgentMode::Primary;
        assert!(
            profile
                .resolve(&crate::skill::SkillRegistry::default(), true)
                .is_err()
        );
    }

    #[test]
    fn agent_profile_session_snapshot_roundtrips_and_old_sessions_load() {
        let session = crate::session::Session::create(None, None);
        let mut value = serde_json::to_value(&session).unwrap();
        let snapshot = ActiveAgentProfile {
            name: "reviewer".into(),
            prompt: "Instructions".into(),
        };
        value["agent_profile"] = serde_json::to_value(&snapshot).unwrap();
        let restored: crate::session::Session = serde_json::from_value(value.clone()).unwrap();
        assert_eq!(restored.agent_profile, Some(snapshot));
        value.as_object_mut().unwrap().remove("agent_profile");
        let old: crate::session::Session = serde_json::from_value(value).unwrap();
        assert_eq!(old.agent_profile, None);
    }
}
