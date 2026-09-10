use super::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AgentProfileScope {
    Global,
    Project,
}

#[derive(Debug, Clone)]
pub struct AgentProfileDraft {
    pub name: String,
    pub description: String,
    pub mode: AgentMode,
    pub model: Option<String>,
    pub effort: Option<String>,
    pub skills: Vec<String>,
    pub prompt: String,
}

pub fn profile_destination(
    scope: AgentProfileScope,
    working_dir: Option<&Path>,
    name: &str,
) -> Result<PathBuf> {
    validate_name(name)?;
    ensure!(name.len() <= 64, "Agent name must be at most 64 characters");
    let directory = match scope {
        AgentProfileScope::Global => crate::storage::jcode_dir()?.join("agents"),
        AgentProfileScope::Project => working_dir
            .context("Project agents require a session working directory")?
            .join(".jcode/agents"),
    };
    ensure!(
        directory.is_absolute(),
        "Agent directory must be an absolute path"
    );
    Ok(directory.join(format!("{name}.md")))
}

pub fn create_profile(
    scope: AgentProfileScope,
    working_dir: Option<&Path>,
    draft: &AgentProfileDraft,
) -> Result<AgentProfile> {
    let result = create_profile_inner(scope, working_dir, draft);
    match &result {
        Ok(profile) => crate::logging::info(&format!(
            "Agent profile created: name={} path={}",
            profile.name,
            profile.path.display()
        )),
        Err(_) => crate::logging::warn("Agent profile creation failed"),
    }
    result
}

fn create_profile_inner(
    scope: AgentProfileScope,
    working_dir: Option<&Path>,
    draft: &AgentProfileDraft,
) -> Result<AgentProfile> {
    use std::io::Write;

    let path = profile_destination(scope, working_dir, &draft.name)?;
    ensure!(
        draft.description.chars().count() <= 1_000 && !draft.description.contains(['\r', '\n']),
        "Agent description must be one line of at most 1,000 characters"
    );
    ensure!(
        draft.prompt.len() <= 65_536,
        "Agent instructions must be at most 64 KiB"
    );
    let yaml = serde_yaml::to_string(&Frontmatter {
        description: draft.description.clone(),
        mode: draft.mode,
        model: draft.model.clone(),
        effort: draft.effort.clone(),
        skills: draft.skills.clone(),
    })?;
    let content = format!("---\n{yaml}---\n\n{}\n", draft.prompt.trim());
    let profile = parse_profile(&path, &content)?;
    if !profile.skills.is_empty() {
        let registry = crate::skill::SkillRegistry::load_for_working_dir_read_only(working_dir)?;
        profile.resolve(&registry, profile.mode == AgentMode::Subagent)?;
    }
    ensure_destination_absent(&path)?;
    let directory = path.parent().context("Agent directory is missing")?;
    let config_dir = directory
        .parent()
        .context("Agent configuration directory is missing")?;
    ensure_plain_directory(config_dir)?;
    ensure_plain_directory(directory)?;
    std::fs::create_dir_all(directory)
        .with_context(|| format!("Cannot create agent directory {}", directory.display()))?;
    ensure_plain_directory(config_dir)?;
    ensure_plain_directory(directory)?;
    let mut staged = tempfile::Builder::new()
        .prefix(".agent-")
        .tempfile_in(directory)?;
    staged.write_all(content.as_bytes())?;
    staged.flush()?;
    staged.as_file().sync_all()?;
    publish_profile(staged, &path)?;
    Ok(profile)
}

fn publish_profile(staged: tempfile::NamedTempFile, path: &Path) -> Result<()> {
    staged
        .persist_noclobber(path)
        .map_err(|error| error.error)
        .with_context(|| {
            format!(
                "Cannot create {} without overwriting an existing file",
                path.display()
            )
        })?;
    Ok(())
}

fn ensure_destination_absent(path: &Path) -> Result<()> {
    match std::fs::symlink_metadata(path) {
        Ok(_) => bail!("Agent destination already exists: {}", path.display()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error).with_context(|| format!("Cannot inspect {}", path.display())),
    }
}

fn ensure_plain_directory(path: &Path) -> Result<()> {
    match std::fs::symlink_metadata(path) {
        Ok(metadata) => {
            ensure!(
                !metadata.is_symlink(),
                "Agent directory must not be a symlink: {}",
                path.display()
            );
            ensure!(
                metadata.is_dir(),
                "Agent directory is not a directory: {}",
                path.display()
            );
            Ok(())
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error).with_context(|| format!("Cannot inspect {}", path.display())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn draft() -> AgentProfileDraft {
        AgentProfileDraft {
            name: "wizard-test".into(),
            description: "Review: \"quoted\" YAML # safely".into(),
            mode: AgentMode::All,
            model: None,
            effort: None,
            skills: vec![],
            prompt: "Review code.\nReport concrete issues.\n---\nKeep body content.".into(),
        }
    }

    struct Home(Option<std::ffi::OsString>);
    impl Home {
        fn set(path: &Path) -> Self {
            let old = std::env::var_os("JCODE_HOME");
            crate::env::set_var("JCODE_HOME", path);
            Self(old)
        }
    }
    impl Drop for Home {
        fn drop(&mut self) {
            match &self.0 {
                Some(old) => crate::env::set_var("JCODE_HOME", old),
                None => crate::env::remove_var("JCODE_HOME"),
            }
        }
    }

    #[test]
    fn agent_profile_create_roundtrips_yaml_and_body() {
        let root = tempfile::tempdir().unwrap();
        let input = draft();
        let profile =
            create_profile(AgentProfileScope::Project, Some(root.path()), &input).unwrap();
        assert_eq!(
            profile.path,
            root.path().join(".jcode/agents/wizard-test.md")
        );
        let contents = std::fs::read_to_string(&profile.path).unwrap();
        let parsed = parse_profile(&profile.path, &contents).unwrap();
        assert_eq!(parsed.description, input.description);
        assert_eq!(parsed.prompt, input.prompt);
        assert_eq!(parsed.mode, AgentMode::All);
        assert!(parsed.model.is_none() && parsed.effort.is_none());
        let mut registry = AgentProfileRegistry::default();
        registry.load_dir(profile.path.parent().unwrap());
        assert!(registry.get(&input.name).is_ok());
    }

    #[test]
    fn agent_profile_create_all_modes_and_optional_model_effort() {
        let root = tempfile::tempdir().unwrap();
        for (name, mode) in [
            ("all", AgentMode::All),
            ("primary", AgentMode::Primary),
            ("worker", AgentMode::Subagent),
        ] {
            let input = AgentProfileDraft {
                name: name.into(),
                mode,
                model: Some("inherit".into()),
                effort: Some("xhigh".into()),
                ..draft()
            };
            let profile =
                create_profile(AgentProfileScope::Project, Some(root.path()), &input).unwrap();
            assert_eq!(profile.mode, mode);
            assert_eq!(profile.model, input.model);
            assert_eq!(profile.effort, input.effort);
        }
    }

    #[test]
    fn agent_profile_create_global_and_preview_do_not_use_cwd_or_write() {
        let _lock = crate::storage::lock_test_env();
        let root = tempfile::tempdir().unwrap();
        let home = root.path().join("global");
        let _home = Home::set(&home);
        let destination =
            profile_destination(AgentProfileScope::Global, None, "wizard-test").unwrap();
        assert_eq!(destination, home.join("agents/wizard-test.md"));
        assert!(!home.exists());
        assert!(profile_destination(AgentProfileScope::Project, None, "wizard-test").is_err());
        assert!(
            profile_destination(
                AgentProfileScope::Project,
                Some(Path::new("relative")),
                "wizard-test"
            )
            .is_err()
        );
        let profile = create_profile(AgentProfileScope::Global, None, &draft()).unwrap();
        assert_eq!(profile.path, destination);
    }

    #[test]
    fn agent_profile_create_rejects_bad_input_before_making_directories() {
        let root = tempfile::tempdir().unwrap();
        for name in [
            "",
            "..",
            "../escape",
            "space name",
            "slash/name",
            &"x".repeat(65),
        ] {
            let input = AgentProfileDraft {
                name: name.into(),
                ..draft()
            };
            assert!(create_profile(AgentProfileScope::Project, Some(root.path()), &input).is_err());
        }
        for input in [
            AgentProfileDraft {
                description: " ".into(),
                ..draft()
            },
            AgentProfileDraft {
                description: "multi\nline".into(),
                ..draft()
            },
            AgentProfileDraft {
                description: "x".repeat(1001),
                ..draft()
            },
            AgentProfileDraft {
                prompt: "x".repeat(65_537),
                ..draft()
            },
            AgentProfileDraft {
                prompt: "".into(),
                ..draft()
            },
            AgentProfileDraft {
                effort: Some("wrong".into()),
                ..draft()
            },
        ] {
            assert!(create_profile(AgentProfileScope::Project, Some(root.path()), &input).is_err());
        }
        assert!(!root.path().join(".jcode").exists());
    }

    #[test]
    fn agent_profile_create_never_clobbers_valid_or_invalid_files() {
        let root = tempfile::tempdir().unwrap();
        let directory = root.path().join(".jcode/agents");
        std::fs::create_dir_all(&directory).unwrap();
        let destination = directory.join("wizard-test.md");
        for content in [
            "invalid profile",
            "---\ndescription: old\n---\nOld instructions.",
        ] {
            std::fs::write(&destination, content).unwrap();
            let error = create_profile(AgentProfileScope::Project, Some(root.path()), &draft())
                .unwrap_err();
            assert!(error.to_string().contains("exist"), "{error:#}");
            assert_eq!(std::fs::read_to_string(&destination).unwrap(), content);
            assert_eq!(std::fs::read_dir(&directory).unwrap().count(), 1);
        }
    }

    #[cfg(unix)]
    #[test]
    fn agent_profile_create_rejects_symlink_destinations_and_directories() {
        use std::os::unix::fs::symlink;
        for (component, dangling) in [
            (".jcode", true),
            (".jcode/agents", true),
            (".jcode/agents/wizard-test.md", true),
            (".jcode", false),
            (".jcode/agents", false),
            (".jcode/agents/wizard-test.md", false),
        ] {
            let root = tempfile::tempdir().unwrap();
            let outside = tempfile::tempdir().unwrap();
            let link = root.path().join(component);
            std::fs::create_dir_all(link.parent().unwrap()).unwrap();
            let target = if dangling {
                outside.path().join("absent")
            } else {
                outside.path().to_path_buf()
            };
            symlink(target, &link).unwrap();
            assert!(
                create_profile(AgentProfileScope::Project, Some(root.path()), &draft()).is_err()
            );
            assert!(std::fs::symlink_metadata(&link).unwrap().is_symlink());
            assert_eq!(std::fs::read_dir(outside.path()).unwrap().count(), 0);
        }
    }

    #[test]
    fn agent_profile_create_concurrent_creation_has_one_winner() {
        let root = tempfile::tempdir().unwrap();
        let barrier = std::sync::Barrier::new(4);
        let successes = std::thread::scope(|s| {
            let tasks: Vec<_> = (0..4)
                .map(|_| {
                    s.spawn(|| {
                        barrier.wait();
                        create_profile(AgentProfileScope::Project, Some(root.path()), &draft())
                            .is_ok()
                    })
                })
                .collect();
            tasks
                .into_iter()
                .map(|task| task.join().unwrap())
                .filter(|success| *success)
                .count()
        });
        assert_eq!(successes, 1);
        let directory = root.path().join(".jcode/agents");
        assert_eq!(std::fs::read_dir(&directory).unwrap().count(), 1);
        let path = directory.join("wizard-test.md");
        assert_eq!(
            parse_profile(&path, &std::fs::read_to_string(&path).unwrap())
                .unwrap()
                .prompt,
            draft().prompt
        );
    }

    #[test]
    fn agent_profile_create_publication_failure_removes_only_owned_staging() {
        use std::io::Write;

        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("wizard-test.md");
        let unrelated = root.path().join(".agent-unrelated");
        std::fs::write(&unrelated, "other writer staging").unwrap();
        let mut staged = tempfile::Builder::new()
            .prefix(".agent-")
            .tempfile_in(root.path())
            .unwrap();
        staged.write_all(b"complete new profile").unwrap();
        staged.flush().unwrap();
        staged.as_file().sync_all().unwrap();
        let staged_path = staged.path().to_path_buf();
        assert!(staged_path.exists());
        std::fs::write(&path, "late competing profile").unwrap();

        let error = publish_profile(staged, &path).unwrap_err();

        assert!(error.to_string().contains("without overwriting"));
        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            "late competing profile"
        );
        assert!(!staged_path.exists());
        assert_eq!(
            std::fs::read_to_string(unrelated).unwrap(),
            "other writer staging"
        );
        assert_eq!(std::fs::read_dir(root.path()).unwrap().count(), 2);
    }

    #[test]
    fn agent_profile_create_rejects_unusable_parent_without_partial_file() {
        let root = tempfile::tempdir().unwrap();
        std::fs::write(root.path().join(".jcode"), "not a directory").unwrap();
        assert!(create_profile(AgentProfileScope::Project, Some(root.path()), &draft()).is_err());
        assert_eq!(
            std::fs::read_to_string(root.path().join(".jcode")).unwrap(),
            "not a directory"
        );
    }

    #[test]
    fn agent_profile_create_revalidates_skills_without_importing() {
        let _lock = crate::storage::lock_test_env();
        let root = tempfile::tempdir().unwrap();
        let project = tempfile::tempdir().unwrap();
        let home = root.path().join("jcode");
        let _home = Home::set(&home);
        let external = crate::storage::user_home_path(".claude/skills/external").unwrap();
        std::fs::create_dir_all(&external).unwrap();
        std::fs::write(
            external.join("SKILL.md"),
            "---\nname: external\ndescription: external\n---\nExternal instructions.",
        )
        .unwrap();
        let skills = project.path().join(".jcode/skills/wizard-review");
        std::fs::create_dir_all(&skills).unwrap();
        std::fs::write(
            skills.join("SKILL.md"),
            "---\nname: wizard-review\ndescription: review\n---\nReview everything.",
        )
        .unwrap();
        let input = AgentProfileDraft {
            prompt: String::new(),
            skills: vec!["wizard-review".into()],
            ..draft()
        };
        let profile =
            create_profile(AgentProfileScope::Project, Some(project.path()), &input).unwrap();
        assert_eq!(profile.skills, input.skills);
        assert!(profile.prompt.is_empty());
        assert!(
            !home.join("skills").exists(),
            "Saving must not import external skills"
        );
        std::fs::remove_file(skills.join("SKILL.md")).unwrap();
        let missing = AgentProfileDraft {
            name: "missing".into(),
            ..input
        };
        let error =
            create_profile(AgentProfileScope::Project, Some(project.path()), &missing).unwrap_err();
        assert!(format!("{error:#}").contains("missing skill"), "{error:#}");
        assert!(!project.path().join(".jcode/agents/missing.md").exists());
        let empty_project = tempfile::tempdir().unwrap();
        assert!(
            create_profile(
                AgentProfileScope::Project,
                Some(empty_project.path()),
                &missing
            )
            .is_err()
        );
        assert!(!empty_project.path().join(".jcode").exists());
    }

    #[test]
    fn agent_profile_create_limits_allow_exact_boundary() {
        let root = tempfile::tempdir().unwrap();
        let input = AgentProfileDraft {
            name: "x".repeat(64),
            description: "é".repeat(1000),
            prompt: "x".repeat(65_536),
            ..draft()
        };
        let profile =
            create_profile(AgentProfileScope::Project, Some(root.path()), &input).unwrap();
        assert_eq!(profile.prompt.len(), 65_536);
        assert_eq!(profile.description.chars().count(), 1000);
    }
}
