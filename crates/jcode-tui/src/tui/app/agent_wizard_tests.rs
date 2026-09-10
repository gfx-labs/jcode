#[test]
fn agent_wizard_picker_offers_creation_without_profile_activation() {
    let mut app = create_test_app();
    let _project = custom_agent_test_project(&mut app);
    app.open_agents_picker();
    let picker = app.inline_interactive_state.as_ref().unwrap();
    assert!(
        picker
            .entries
            .iter()
            .any(|entry| entry.name.starts_with("Create new agent"))
    );
    assert!(app.pending_agent_profile.is_none());
    assert!(!app.is_processing);
}

#[test]
fn agent_wizard_create_command_does_not_activate_a_profile() {
    let mut app = create_test_app();
    let _project = custom_agent_test_project(&mut app);
    app.is_remote = true;
    assert!(super::commands::handle_agents_command(
        &mut app,
        "/agents create"
    ));
    assert!(
        app.pending_agent_profile.is_none(),
        "create must open a wizard, not activate a profile named create"
    );
}

fn agent_wizard_key(app: &mut App, code: KeyCode) {
    app.handle_key(code, KeyModifiers::NONE).unwrap();
}

fn agent_wizard_manual_review(app: &mut App, name: &str, body: &str) {
    super::commands::handle_agents_command(app, "/agents create");
    agent_wizard_key(app, KeyCode::Enter);
    app.handle_paste(name.into());
    agent_wizard_key(app, KeyCode::Enter);
    app.handle_paste("Review code safely".into());
    agent_wizard_key(app, KeyCode::Enter);
    agent_wizard_key(app, KeyCode::Enter);
    agent_wizard_key(app, KeyCode::Enter);
    agent_wizard_key(app, KeyCode::Down);
    agent_wizard_key(app, KeyCode::Enter);
    app.handle_paste(body.into());
    agent_wizard_key(app, KeyCode::Enter);
    agent_wizard_key(app, KeyCode::Enter);
    agent_wizard_key(app, KeyCode::Enter);
    agent_wizard_key(app, KeyCode::Enter);
}

#[test]
fn agent_wizard_manual_save_is_explicit_and_preserves_chat_and_composer() {
    let mut app = create_test_app();
    let project = custom_agent_test_project(&mut app);
    std::fs::write(
        project.path().join(".jcode/agents/unrelated-broken.md"),
        "---\ninvalid YAML: [\n---\n",
    )
    .unwrap();
    app.input = "Unsent composer text".into();
    app.cursor_pos = 6;
    let model = app.provider.model();
    let transcript_len = app.display_messages.len();
    let path = project.path().join(".jcode/agents/wizard-manual.md");
    agent_wizard_manual_review(
        &mut app,
        "wizard-manual",
        "Inspect changes.\nReport issues.",
    );
    assert!(!path.exists());
    assert_eq!(app.display_messages.len(), transcript_len);
    assert_eq!(app.input, "Unsent composer text");
    assert_eq!(app.cursor_pos, 6);
    agent_wizard_key(&mut app, KeyCode::Enter);
    let text = std::fs::read_to_string(&path).expect("confirmed Save creates the profile");
    assert!(text.contains("Inspect changes.\nReport issues."));
    assert_eq!(app.provider.model(), model);
    assert!(app.session.agent_profile.is_none());
    assert_eq!(app.display_messages.len(), transcript_len);
    assert_eq!(app.input, "Unsent composer text");
    let picker = app.inline_interactive_state.as_ref().unwrap();
    assert_eq!(
        picker.entries[picker.filtered[picker.selected]].name,
        "wizard-manual"
    );
    assert!(!app.pending_turn && !app.is_processing);
}

#[test]
fn agent_wizard_cancel_keeps_composer_and_creates_no_files() {
    let mut app = create_test_app();
    let project = custom_agent_test_project(&mut app);
    app.input = "Keep this".into();
    app.cursor_pos = 4;
    let before = app.display_messages.len();
    super::commands::handle_agents_command(&mut app, "/agents create");
    agent_wizard_key(&mut app, KeyCode::Esc);
    agent_wizard_key(&mut app, KeyCode::Enter);
    agent_wizard_key(&mut app, KeyCode::Esc);
    agent_wizard_key(&mut app, KeyCode::Down);
    agent_wizard_key(&mut app, KeyCode::Enter);
    assert_eq!(app.input, "Keep this");
    assert_eq!(app.cursor_pos, 4);
    assert_eq!(app.display_messages.len(), before);
    assert_eq!(
        std::fs::read_dir(project.path().join(".jcode/agents"))
            .unwrap()
            .count(),
        0
    );
    assert!(!app.pending_turn && !app.is_processing);
}

#[test]
fn agent_wizard_busy_entry_and_reserved_profile_commands_are_distinct() {
    let mut app = create_test_app();
    let project = custom_agent_test_project(&mut app);
    for guard in 0..5 {
        app.is_processing = guard == 0;
        app.pending_turn = guard == 1;
        app.remote_model_switch_in_flight = guard == 2;
        app.pending_agent_profile = (guard == 3).then_some(None);
        app.pending_model_switch = (guard == 4).then_some("pending-model".into());
        super::commands::handle_agents_command(&mut app, "/agents create");
        assert!(app.agent_wizard.is_none());
    }
    app.is_processing = false;
    app.pending_turn = false;
    app.remote_model_switch_in_flight = false;
    app.pending_agent_profile = None;
    app.pending_model_switch = None;
    for name in ["create", "clear", "review"] {
        std::fs::write(
            project.path().join(format!(".jcode/agents/{name}.md")),
            "---\ndescription: Existing profile\n---\nUse this profile.",
        )
        .unwrap();
        super::commands::handle_agents_command(&mut app, &format!("/agents use {name}"));
        assert!(app.agent_wizard.is_none());
        assert_eq!(app.active_agent_profile_name(), Some(name));
    }
}

#[test]
fn agent_wizard_picker_keyboard_entry_preserves_composer() {
    let mut app = create_test_app();
    let _project = custom_agent_test_project(&mut app);
    app.input = "Preserved text".into();
    app.cursor_pos = 4;
    app.open_agents_picker();
    agent_wizard_key(&mut app, KeyCode::Enter);
    assert_eq!(
        app.agent_wizard.as_ref().unwrap().step,
        super::agent_wizard::Step::Location
    );
    assert_eq!(app.input, "Preserved text");
    assert_eq!(app.cursor_pos, 4);
}

#[test]
fn agent_wizard_publish_race_preserves_existing_file_and_draft() {
    let mut app = create_test_app();
    let project = custom_agent_test_project(&mut app);
    agent_wizard_manual_review(&mut app, "wizard-race", "New instructions");
    let path = project.path().join(".jcode/agents/wizard-race.md");
    std::fs::write(&path, "Original bytes").unwrap();
    agent_wizard_key(&mut app, KeyCode::Enter);
    assert_eq!(std::fs::read_to_string(path).unwrap(), "Original bytes");
    assert_eq!(
        app.agent_wizard.as_ref().unwrap().step,
        super::agent_wizard::Step::Review
    );
    assert_eq!(
        app.agent_wizard.as_ref().unwrap().draft.prompt,
        "New instructions"
    );
}

#[test]
fn agent_wizard_name_validation_rejects_paths_and_existing_invalid_profiles() {
    let mut app = create_test_app();
    let project = custom_agent_test_project(&mut app);
    std::fs::write(
        project.path().join(".jcode/agents/broken.md"),
        "bad frontmatter",
    )
    .unwrap();
    super::commands::handle_agents_command(&mut app, "/agents create");
    agent_wizard_key(&mut app, KeyCode::Enter);
    for name in ["../escape", "broken", "", "spaces not allowed"] {
        app.handle_key(KeyCode::Char('a'), KeyModifiers::CONTROL)
            .unwrap();
        app.handle_key(KeyCode::Char('k'), KeyModifiers::CONTROL)
            .unwrap();
        app.handle_paste(name.into());
        agent_wizard_key(&mut app, KeyCode::Enter);
        assert_eq!(
            app.agent_wizard.as_ref().unwrap().step,
            super::agent_wizard::Step::Name
        );
    }
}

#[test]
fn agent_wizard_unicode_edit_back_and_limits_do_not_touch_composer() {
    let mut app = create_test_app();
    let _project = custom_agent_test_project(&mut app);
    agent_wizard_manual_review(&mut app, "wizard-edit", "A中é");
    for _ in 0..4 {
        agent_wizard_key(&mut app, KeyCode::Esc);
    }
    assert_eq!(
        app.agent_wizard.as_ref().unwrap().step,
        super::agent_wizard::Step::Instructions
    );
    agent_wizard_key(&mut app, KeyCode::Backspace);
    app.handle_key(KeyCode::Enter, KeyModifiers::SHIFT).unwrap();
    app.handle_paste("line".into());
    app.handle_paste("x".repeat(65537));
    agent_wizard_key(&mut app, KeyCode::Enter);
    assert_eq!(app.agent_wizard.as_ref().unwrap().draft.prompt, "A中\nline");
    assert!(app.input.is_empty());
}

#[test]
fn agent_wizard_model_and_effort_choices_are_draft_only() {
    let mut app = create_test_app();
    let _project = custom_agent_test_project(&mut app);
    app.is_remote = true;
    app.remote_provider_name = Some("OpenAI".into());
    app.remote_provider_model = Some("gpt-5".into());
    app.remote_model_options = vec![crate::provider::ModelRoute {
        model: "gpt-5".into(),
        provider: "OpenAI".into(),
        api_method: "openai-api".into(),
        available: true,
        detail: "test route".into(),
        cheapness: None,
    }];
    agent_wizard_manual_review(&mut app, "wizard-model", "Instructions");
    for _ in 0..3 {
        agent_wizard_key(&mut app, KeyCode::Esc);
    }
    agent_wizard_key(&mut app, KeyCode::Down);
    agent_wizard_key(&mut app, KeyCode::Enter);
    assert_eq!(
        app.agent_wizard.as_ref().unwrap().draft.model.as_deref(),
        Some("openai-api:gpt-5")
    );
    agent_wizard_key(&mut app, KeyCode::Down);
    agent_wizard_key(&mut app, KeyCode::Enter);
    assert!(app.agent_wizard.as_ref().unwrap().draft.effort.is_some());
    assert_eq!(app.remote_provider_model.as_deref(), Some("gpt-5"));
    assert!(app.pending_model_switch.is_none() && app.pending_route_selection.is_none());
    assert!(app.pending_reasoning_effort.is_none());
    let draft_model = app.agent_wizard.as_ref().unwrap().draft.model.clone();
    let draft_effort = app.agent_wizard.as_ref().unwrap().draft.effort.clone();
    agent_wizard_key(&mut app, KeyCode::Esc);
    agent_wizard_key(&mut app, KeyCode::Esc);
    agent_wizard_key(&mut app, KeyCode::Enter);
    agent_wizard_key(&mut app, KeyCode::Enter);
    assert_eq!(app.agent_wizard.as_ref().unwrap().draft.model, draft_model);
    assert_eq!(
        app.agent_wizard.as_ref().unwrap().draft.effort,
        draft_effort
    );
}

#[test]
fn agent_wizard_back_retains_usage_and_worker_activation_is_disabled() {
    let mut app = create_test_app();
    let project = custom_agent_test_project(&mut app);
    agent_wizard_manual_review(&mut app, "wizard-worker", "Instructions");
    for _ in 0..6 {
        agent_wizard_key(&mut app, KeyCode::Esc);
    }
    agent_wizard_key(&mut app, KeyCode::Down);
    agent_wizard_key(&mut app, KeyCode::Down);
    agent_wizard_key(&mut app, KeyCode::Enter);
    agent_wizard_key(&mut app, KeyCode::Esc);
    agent_wizard_key(&mut app, KeyCode::Enter);
    assert_eq!(
        app.agent_wizard.as_ref().unwrap().draft.mode,
        crate::agent_profile::AgentMode::Subagent
    );
    agent_wizard_key(&mut app, KeyCode::Down);
    for _ in 0..5 {
        agent_wizard_key(&mut app, KeyCode::Enter);
    }
    agent_wizard_key(&mut app, KeyCode::Down);
    agent_wizard_key(&mut app, KeyCode::Enter);
    assert!(
        !project
            .path()
            .join(".jcode/agents/wizard-worker.md")
            .exists()
    );
    assert_eq!(
        app.agent_wizard.as_ref().unwrap().step,
        super::agent_wizard::Step::Review
    );
    assert!(app.session.agent_profile.is_none());
}

#[test]
fn agent_wizard_skills_only_profile_saves_names_not_skill_body() {
    let mut app = create_test_app();
    let project = custom_agent_test_project(&mut app);
    let skill_dir = project.path().join(".jcode/skills/wizard-skill");
    std::fs::create_dir_all(&skill_dir).unwrap();
    std::fs::write(
        skill_dir.join("SKILL.md"),
        "---\nname: wizard-skill\ndescription: Fixture skill\n---\nPRIVATE_SKILL_BODY",
    )
    .unwrap();
    agent_wizard_manual_review(&mut app, "wizard-skills", "");
    assert_eq!(
        app.agent_wizard.as_ref().unwrap().step,
        super::agent_wizard::Step::Skills
    );
    app.handle_paste("wizard-skill".into());
    agent_wizard_key(&mut app, KeyCode::Char(' '));
    agent_wizard_key(&mut app, KeyCode::Enter);
    agent_wizard_key(&mut app, KeyCode::Enter);
    let text =
        std::fs::read_to_string(project.path().join(".jcode/agents/wizard-skills.md")).unwrap();
    assert!(text.contains("wizard-skill"));
    assert!(!text.contains("PRIVATE_SKILL_BODY"));
}

#[test]
fn agent_wizard_help_and_completion_expose_create() {
    let app = create_test_app();
    assert!(
        app.command_help("agents")
            .unwrap()
            .contains("/agents create")
    );
    assert!(
        app.get_suggestions_for("/agents cr")
            .iter()
            .any(|(command, _)| command == "/agents create")
    );
}

#[test]
fn agent_wizard_narrow_and_large_rendering_keeps_save_and_scrollable_content() {
    let mut app = create_test_app();
    let _project = custom_agent_test_project(&mut app);
    agent_wizard_manual_review(
        &mut app,
        "wizard-render",
        &format!("{}END_OF_INSTRUCTIONS", "中x\n".repeat(6000)),
    );
    for (width, height) in [(1, 1), (12, 6), (38, 18), (100, 32)] {
        let backend = ratatui::backend::TestBackend::new(width, height);
        let mut terminal = ratatui::Terminal::new(backend).unwrap();
        terminal
            .draw(|frame| crate::tui::ui::draw(frame, &app))
            .unwrap();
        if width >= 12 {
            let text = terminal
                .backend()
                .buffer()
                .content()
                .iter()
                .map(|cell| cell.symbol())
                .collect::<String>();
            assert!(text.contains("Save"));
        }
    }
    for _ in 0..900 {
        agent_wizard_key(&mut app, KeyCode::PageDown);
    }
    let backend = ratatui::backend::TestBackend::new(100, 32);
    let mut terminal = ratatui::Terminal::new(backend).unwrap();
    terminal
        .draw(|frame| crate::tui::ui::draw(frame, &app))
        .unwrap();
    let text = terminal
        .backend()
        .buffer()
        .content()
        .iter()
        .map(|cell| cell.symbol())
        .collect::<String>();
    assert!(text.contains("END_OF_INSTRUCTIONS"));
}

#[test]
fn agent_wizard_narrow_error_keeps_back_and_cancel_hints_visible() {
    let mut app = create_test_app();
    let project = custom_agent_test_project(&mut app);
    let name = "wizard-navigation";
    std::fs::write(
        project.path().join(format!(".jcode/agents/{name}.md")),
        "Existing instructions",
    )
    .unwrap();
    super::commands::handle_agents_command(&mut app, "/agents create");
    agent_wizard_key(&mut app, KeyCode::Enter);
    app.handle_paste(name.into());
    agent_wizard_key(&mut app, KeyCode::Enter);
    for (width, height) in [(40, 12), (100, 32)] {
        let mut terminal =
            ratatui::Terminal::new(ratatui::backend::TestBackend::new(width, height)).unwrap();
        terminal
            .draw(|frame| crate::tui::ui::draw(frame, &app))
            .unwrap();
        let text = terminal
            .backend()
            .buffer()
            .content()
            .iter()
            .map(|cell| cell.symbol())
            .collect::<String>();
        assert!(text.contains("2/11 Name"));
        assert!(text.contains("wizard-navigation▏"));
        assert!(text.contains("Already exists:"));
        assert!(text.contains("Esc back"), "{width}x{height}: {text}");
        assert!(text.contains("Ctrl+C cancel"), "{width}x{height}: {text}");
    }
}

#[test]
fn agent_wizard_short_review_can_scroll_through_destination_and_instruction_tail() {
    let mut app = create_test_app();
    let _project = custom_agent_test_project(&mut app);
    agent_wizard_manual_review(&mut app, "wizard-small", "Instruction body\nTAIL");
    let mut terminal = ratatui::Terminal::new(ratatui::backend::TestBackend::new(12, 6)).unwrap();
    let mut detail = String::new();
    for _ in 0..120 {
        terminal
            .draw(|frame| crate::tui::ui::draw(frame, &app))
            .unwrap();
        let buffer = terminal.backend().buffer();
        let row = (1..11).map(|x| buffer[(x, 2)].symbol()).collect::<String>();
        detail.push_str(row.trim_end());
        agent_wizard_key(&mut app, KeyCode::PageDown);
    }
    assert!(
        detail.contains(".jcode/agents/wizard-small.md"),
        "destination unreachable: {detail}"
    );
    assert!(
        detail.contains("TAIL"),
        "instruction tail unreachable: {detail}"
    );
}

#[test]
fn agent_wizard_debug_frame_advances_without_recording_private_draft() {
    if std::env::var_os("JCODE_TEST_WIZARD_FRAME_CHILD").is_none() {
        let home = tempfile::tempdir().unwrap();
        let output = std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "tui::app::tests::agent_wizard_debug_frame_advances_without_recording_private_draft", "--nocapture"])
            .env("JCODE_TEST_WIZARD_FRAME_CHILD", "1")
            .env("JCODE_HOME", home.path())
            .env("JCODE_IDLE_ANIMATION", "1")
            .output().unwrap();
        assert!(
            output.status.success(),
            "{}\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        return;
    }
    crate::perf::pin_full_profile_for_tests();
    let mut app = create_test_app();
    let _project = custom_agent_test_project(&mut app);
    crate::tui::visual_debug::enable();
    let mut terminal = ratatui::Terminal::new(ratatui::backend::TestBackend::new(80, 24)).unwrap();
    terminal
        .draw(|frame| crate::tui::ui::draw(frame, &app))
        .unwrap();
    let previous = crate::tui::visual_debug::latest_frame().unwrap().frame_id;
    app.input = "PRIVATE_COMPOSER".into();
    agent_wizard_manual_review(&mut app, "wizard-private-frame", "PRIVATE_INSTRUCTIONS");
    terminal
        .draw(|frame| crate::tui::ui::draw(frame, &app))
        .unwrap();
    let capture = crate::tui::visual_debug::latest_frame().unwrap();
    assert!(
        capture.frame_id > previous,
        "wizard must replace the previous chat capture"
    );
    assert!(
        capture
            .render_order
            .iter()
            .any(|phase| phase == "agent_wizard")
    );
    assert!(capture.rendered_text.status_line.contains("Review"));
    assert!(capture.render_timing.is_some());
    let json = serde_json::to_string(&capture).unwrap();
    assert!(!json.contains("PRIVATE_COMPOSER"));
    assert!(!json.contains("PRIVATE_INSTRUCTIONS"));
    assert!(!json.contains("wizard-private-frame"));
    let previous = capture.frame_id;
    agent_wizard_key(&mut app, KeyCode::Esc);
    terminal
        .draw(|frame| crate::tui::ui::draw(frame, &app))
        .unwrap();
    assert!(crate::tui::visual_debug::latest_frame().unwrap().frame_id > previous);
    crate::tui::visual_debug::disable();
}

#[test]
fn agent_wizard_remote_debug_keys_request_redraw_without_composer_changes() {
    let mut app = create_test_app();
    let _project = custom_agent_test_project(&mut app);
    let rt = tokio::runtime::Runtime::new().unwrap();
    let _guard = rt.enter();
    let mut remote = crate::tui::backend::RemoteConnection::dummy();
    let mut state = super::remote::RemoteRunState::default();
    let mut terminal = ratatui::Terminal::new(ratatui::backend::TestBackend::new(80, 24)).unwrap();
    app.open_agent_wizard();
    let input = app.input.clone();
    for command in [
        "keys:enter",
        "keys:x",
        "keys:ctrl+c",
        "keys:down",
        "keys:enter",
    ] {
        let (_, needs_redraw) = rt
            .block_on(super::remote::handle_remote_event(
                &mut app,
                &mut terminal,
                &mut remote,
                &mut state,
                crate::tui::backend::RemoteRead::Event(
                    crate::protocol::ServerEvent::ClientDebugRequest {
                        id: 900,
                        command: command.into(),
                    },
                ),
            ))
            .unwrap();
        assert!(
            needs_redraw,
            "{command} changed the wizard but requested no paint"
        );
        assert_eq!(app.input, input);
    }
    assert!(app.agent_wizard.is_none());
}

#[test]
fn agent_wizard_suspends_hidden_idle_animation() {
    if std::env::var_os("JCODE_TEST_WIZARD_IDLE_CHILD").is_none() {
        let home = tempfile::tempdir().unwrap();
        let output = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "tui::app::tests::agent_wizard_suspends_hidden_idle_animation",
                "--nocapture",
            ])
            .env("JCODE_TEST_WIZARD_IDLE_CHILD", "1")
            .env("JCODE_HOME", home.path())
            .env("JCODE_IDLE_ANIMATION", "1")
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        return;
    }
    crate::perf::pin_full_profile_for_tests();
    let mut app = create_test_app();
    app.display_messages.clear();
    app.push_display_message(DisplayMessage::system("Idle fixture"));
    app.last_user_interaction = Some(std::time::Instant::now());
    app.open_agent_wizard();
    assert!(app.agent_wizard.is_some());
    assert!(
        !crate::tui::idle_donut_active(&app),
        "wizard owns the full viewport"
    );
    app.agent_wizard = None;
    assert!(
        crate::tui::idle_donut_active(&app),
        "fixture must exercise an eligible animation: policy={:?}, config={}, focused={}, welcome={}, startup={}, processing={}",
        crate::perf::tui_policy(),
        crate::config::config().display.idle_animation,
        app.client_focused(),
        crate::tui::TuiState::onboarding_welcome_active(&app),
        app.remote_startup_phase_active(),
        app.is_processing
    );
}

#[test]
fn agent_wizard_reserved_command_requires_acknowledgement_per_destination() {
    let mut app = create_test_app();
    let _project = custom_agent_test_project(&mut app);
    app.open_agent_wizard();
    agent_wizard_key(&mut app, KeyCode::Enter);
    for name in ["create", "clear"] {
        app.handle_key(KeyCode::Char('u'), KeyModifiers::CONTROL)
            .unwrap();
        app.handle_paste(name.into());
        agent_wizard_key(&mut app, KeyCode::Enter);
        assert_eq!(
            app.agent_wizard.as_ref().unwrap().step,
            super::agent_wizard::Step::Name
        );
        agent_wizard_key(&mut app, KeyCode::Enter);
        assert_eq!(
            app.agent_wizard.as_ref().unwrap().step,
            super::agent_wizard::Step::Description
        );
        agent_wizard_key(&mut app, KeyCode::Esc);
    }
}

#[test]
fn agent_wizard_project_shadow_requires_acknowledgement_for_each_name() {
    if std::env::var_os("JCODE_TEST_WIZARD_SHADOW_CHILD").is_none() {
        let home = tempfile::tempdir().unwrap();
        let output = std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "tui::app::tests::agent_wizard_project_shadow_requires_acknowledgement_for_each_name", "--nocapture"])
            .env("JCODE_TEST_WIZARD_SHADOW_CHILD", "1")
            .env("JCODE_HOME", home.path())
            .output().unwrap();
        assert!(
            output.status.success(),
            "{}\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        return;
    }
    let home = PathBuf::from(std::env::var_os("JCODE_HOME").unwrap());
    std::fs::create_dir_all(home.join("agents")).unwrap();
    for name in ["wizard-shadow-one", "wizard-shadow-two"] {
        std::fs::write(
            home.join(format!("agents/{name}.md")),
            "---\ndescription: Global fixture\n---\nGlobal instructions",
        )
        .unwrap();
    }
    let mut app = create_test_app();
    let _project = custom_agent_test_project(&mut app);
    app.open_agent_wizard();
    agent_wizard_key(&mut app, KeyCode::Enter);
    for name in ["wizard-shadow-one", "wizard-shadow-two"] {
        app.handle_key(KeyCode::Char('u'), KeyModifiers::CONTROL)
            .unwrap();
        app.handle_paste(name.into());
        agent_wizard_key(&mut app, KeyCode::Enter);
        assert_eq!(
            app.agent_wizard.as_ref().unwrap().step,
            super::agent_wizard::Step::Name
        );
        agent_wizard_key(&mut app, KeyCode::Enter);
        assert_eq!(
            app.agent_wizard.as_ref().unwrap().step,
            super::agent_wizard::Step::Description
        );
        agent_wizard_key(&mut app, KeyCode::Esc);
    }
}

#[test]
fn agent_wizard_shared_text_paste_never_creates_chat_placeholders_or_previews() {
    let mut app = create_test_app();
    let _project = custom_agent_test_project(&mut app);
    app.input = "Unsent".into();
    app.cursor_pos = 2;
    agent_wizard_manual_review(&mut app, "wizard-paste", "Start");
    for _ in 0..4 {
        agent_wizard_key(&mut app, KeyCode::Esc);
    }
    super::input::handle_text_paste(&mut app, "\nline".repeat(50));
    super::input::insert_input_text(&mut app, "/model");
    agent_wizard_key(&mut app, KeyCode::Enter);
    assert_eq!(
        app.agent_wizard.as_ref().unwrap().draft.prompt,
        format!("Start{}/model", "\nline".repeat(50))
    );
    assert_eq!(app.input, "Unsent");
    assert!(app.inline_interactive_state.is_none());
    assert!(!app.pending_turn && !app.is_processing);
}

#[test]
fn agent_wizard_async_clipboard_does_not_attach_images_or_edit_chat() {
    let mut app = create_test_app();
    let _project = custom_agent_test_project(&mut app);
    agent_wizard_manual_review(&mut app, "wizard-clipboard", "Start");
    for _ in 0..4 {
        agent_wizard_key(&mut app, KeyCode::Esc);
    }
    let session_id = app.active_client_session_id().unwrap().to_string();
    let before = app.display_messages.len();
    let images = app.pending_images.len();
    app.handle_clipboard_paste_completed(crate::bus::ClipboardPasteCompleted {
        session_id: session_id.clone(),
        kind: crate::bus::ClipboardPasteKind::Smart,
        content: crate::bus::ClipboardPasteContent::Text(" pasted".into()),
    });
    app.handle_clipboard_paste_completed(crate::bus::ClipboardPasteCompleted {
        session_id,
        kind: crate::bus::ClipboardPasteKind::Smart,
        content: crate::bus::ClipboardPasteContent::Image {
            media_type: "image/png".into(),
            base64_data: "not-an-image".into(),
        },
    });
    agent_wizard_key(&mut app, KeyCode::Enter);
    assert_eq!(
        app.agent_wizard.as_ref().unwrap().draft.prompt,
        "Start pasted"
    );
    assert_eq!(app.pending_images.len(), images);
    assert_eq!(app.display_messages.len(), before);
    assert!(app.input.is_empty());
}

#[test]
fn agent_wizard_async_clipboard_session_dismissal_consumes_paste_without_chat_fallback() {
    let mut app = create_test_app();
    let _project = custom_agent_test_project(&mut app);
    app.input = "Preserved composer".into();
    app.cursor_pos = 4;
    app.open_agent_wizard();
    let old_session = app.session.id.clone();
    app.session.id = "replacement-session".into();
    let before = app.display_messages.len();
    assert!(
        !app.handle_clipboard_paste_completed(crate::bus::ClipboardPasteCompleted {
            session_id: old_session,
            kind: crate::bus::ClipboardPasteKind::Smart,
            content: crate::bus::ClipboardPasteContent::Text("Old clipboard".into()),
        })
    );
    assert!(app.agent_wizard.is_some());
    assert!(
        app.handle_clipboard_paste_completed(crate::bus::ClipboardPasteCompleted {
            session_id: app.session.id.clone(),
            kind: crate::bus::ClipboardPasteKind::Smart,
            content: crate::bus::ClipboardPasteContent::Text("Must not reach chat".into()),
        })
    );
    assert!(app.agent_wizard.is_none());
    assert_eq!(app.input, "Preserved composer");
    assert_eq!(app.cursor_pos, 4);
    assert_eq!(app.display_messages.len(), before);
    assert!(!app.pending_turn && !app.is_processing);
}

fn agent_wizard_generation_choice(app: &mut App) {
    super::commands::handle_agents_command(app, "/agents create");
    agent_wizard_key(app, KeyCode::Enter);
    app.handle_paste("wizard-generated".into());
    agent_wizard_key(app, KeyCode::Enter);
    app.handle_paste("Review code".into());
    agent_wizard_key(app, KeyCode::Enter);
    app.handle_paste("Find concrete correctness errors".into());
    agent_wizard_key(app, KeyCode::Enter);
    agent_wizard_key(app, KeyCode::Enter);
}

#[test]
fn agent_wizard_generation_disconnect_and_session_change_reject_late_results() {
    let mut app = create_test_app();
    let rt = tokio::runtime::Runtime::new().unwrap();
    rt.block_on(async {
        let _project = custom_agent_test_project(&mut app);
        app.is_remote = true;
        let mut remote = crate::tui::backend::RemoteConnection::dummy();
        let peer = remote.take_dummy_peer().unwrap();
        use tokio::io::{AsyncBufReadExt, BufReader};
        let mut reader = BufReader::new(peer);
        agent_wizard_generation_choice(&mut app);
        for changed_session in [false, true] {
            super::remote::handle_remote_key(
                &mut app,
                KeyCode::Enter,
                KeyModifiers::NONE,
                &mut remote,
            )
            .await
            .unwrap();
            let mut line = String::new();
            tokio::time::timeout(Duration::from_secs(2), reader.read_line(&mut line))
                .await
                .unwrap()
                .unwrap();
            let id = serde_json::from_str::<crate::protocol::Request>(&line)
                .unwrap()
                .id();
            if changed_session {
                app.session.id = "different-session".into();
            } else {
                app.agent_wizard_disconnected();
            }
            app.handle_server_event(
                crate::protocol::ServerEvent::AgentInstructionsGenerated {
                    id,
                    text: Some("LATE".into()),
                    model: "model".into(),
                    provider_name: "provider".into(),
                    error: None,
                },
                &mut remote,
            );
            assert!(app.agent_wizard.as_ref().unwrap().draft.prompt.is_empty());
            assert_eq!(
                app.agent_wizard.as_ref().unwrap().step,
                super::agent_wizard::Step::InstructionChoice
            );
        }
        app.poll_agent_wizard();
        assert!(app.agent_wizard.is_none());
        assert!(!app.pending_turn && !app.is_processing);
    });
}

#[test]
fn agent_wizard_local_unavailable_generator_returns_to_manual_without_turn() {
    let mut app = create_test_app();
    let _project = custom_agent_test_project(&mut app);
    let rt = tokio::runtime::Runtime::new().unwrap();
    rt.block_on(async {
        let before = app.display_messages.len();
        agent_wizard_generation_choice(&mut app);
        agent_wizard_key(&mut app, KeyCode::Enter);
        assert_eq!(
            app.agent_wizard.as_ref().unwrap().step,
            super::agent_wizard::Step::InstructionChoice,
            "unsupported providers must fail synchronously before a generation task is spawned"
        );
        assert_eq!(
            app.agent_wizard.as_ref().unwrap().step,
            super::agent_wizard::Step::InstructionChoice
        );
        assert_eq!(app.display_messages.len(), before);
        assert!(!app.pending_turn && !app.is_processing);
    });
}

#[test]
fn agent_wizard_session_change_before_key_or_paste_never_saves_or_edits_composer() {
    for paste in [false, true] {
        let mut app = create_test_app();
        let project = custom_agent_test_project(&mut app);
        app.input = "Keep composer".into();
        agent_wizard_manual_review(&mut app, "wizard-session-guard", "Instructions");
        app.session.id = "replacement-session".into();
        if paste {
            app.handle_paste("must not reach composer".into());
        } else {
            agent_wizard_key(&mut app, KeyCode::Enter);
        }
        assert!(
            !project
                .path()
                .join(".jcode/agents/wizard-session-guard.md")
                .exists()
        );
        assert!(app.agent_wizard.is_none());
        assert_eq!(app.input, "Keep composer");
    }
}

#[test]
fn agent_wizard_save_waits_for_new_pending_model_change() {
    let mut app = create_test_app();
    let project = custom_agent_test_project(&mut app);
    agent_wizard_manual_review(&mut app, "wizard-model-guard", "Instructions");
    app.pending_model_switch = Some("replacement-model".into());
    agent_wizard_key(&mut app, KeyCode::Enter);
    assert!(
        !project
            .path()
            .join(".jcode/agents/wizard-model-guard.md")
            .exists()
    );
    assert!(app.agent_wizard.is_some());
}

#[test]
fn agent_wizard_keep_editing_after_canceling_generation_has_manual_fallback() {
    let mut app = create_test_app();
    let _project = custom_agent_test_project(&mut app);
    app.is_remote = true;
    agent_wizard_generation_choice(&mut app);
    agent_wizard_key(&mut app, KeyCode::Enter);
    app.handle_key(KeyCode::Char('c'), KeyModifiers::CONTROL)
        .unwrap();
    agent_wizard_key(&mut app, KeyCode::Enter);
    assert_eq!(
        app.agent_wizard.as_ref().unwrap().step,
        super::agent_wizard::Step::InstructionChoice
    );
    agent_wizard_key(&mut app, KeyCode::Down);
    agent_wizard_key(&mut app, KeyCode::Enter);
    assert_eq!(
        app.agent_wizard.as_ref().unwrap().step,
        super::agent_wizard::Step::Instructions
    );
}

#[test]
fn agent_wizard_mouse_never_operates_on_hidden_chat_overlays() {
    let mut app = create_test_app();
    let _project = custom_agent_test_project(&mut app);
    app.open_agent_wizard();
    app.help_scroll = Some(0);
    let scroll_only = app.handle_mouse_event(crossterm::event::MouseEvent {
        kind: crossterm::event::MouseEventKind::ScrollDown,
        column: 1,
        row: 1,
        modifiers: KeyModifiers::NONE,
    });
    assert!(
        !scroll_only,
        "wizard must consume mouse input before hidden overlays"
    );
    assert_eq!(app.help_scroll, Some(0));
}

#[test]
fn agent_wizard_repeated_cancel_keeps_original_step() {
    let mut app = create_test_app();
    let _project = custom_agent_test_project(&mut app);
    app.open_agent_wizard();
    agent_wizard_key(&mut app, KeyCode::Enter);
    for _ in 0..2 {
        app.handle_key(KeyCode::Char('c'), KeyModifiers::CONTROL)
            .unwrap();
    }
    agent_wizard_key(&mut app, KeyCode::Enter);
    assert_eq!(
        app.agent_wizard.as_ref().unwrap().step,
        super::agent_wizard::Step::Name
    );
}

#[test]
fn agent_wizard_unicode_word_delete_preserves_utf8_boundaries() {
    let mut app = create_test_app();
    let _project = custom_agent_test_project(&mut app);
    agent_wizard_generation_choice(&mut app);
    agent_wizard_key(&mut app, KeyCode::Down);
    agent_wizard_key(&mut app, KeyCode::Enter);
    app.handle_paste("first\u{3000}second".into());
    app.handle_key(KeyCode::Char('w'), KeyModifiers::CONTROL)
        .unwrap();
    agent_wizard_key(&mut app, KeyCode::Enter);
    assert_eq!(
        app.agent_wizard.as_ref().unwrap().draft.prompt,
        "first\u{3000}"
    );
}

#[test]
fn agent_wizard_rejects_global_name_hidden_by_project_file() {
    let mut app = create_test_app();
    let project = custom_agent_test_project(&mut app);
    let name = "wizard-shadowed-global";
    let project_path = project.path().join(format!(".jcode/agents/{name}.md"));
    std::fs::write(&project_path, "Malformed existing project profile").unwrap();
    app.open_agent_wizard();
    agent_wizard_key(&mut app, KeyCode::Down);
    agent_wizard_key(&mut app, KeyCode::Enter);
    app.handle_paste(name.into());
    agent_wizard_key(&mut app, KeyCode::Enter);
    assert_eq!(
        app.agent_wizard.as_ref().unwrap().step,
        super::agent_wizard::Step::Name
    );
    assert!(app.pending_agent_profile.is_none());
    assert_eq!(
        std::fs::read_to_string(project_path).unwrap(),
        "Malformed existing project profile"
    );
}

#[test]
fn agent_wizard_generation_cancel_and_old_daemon_error_stay_private() {
    let mut app = create_test_app();
    let rt = tokio::runtime::Runtime::new().unwrap();
    rt.block_on(async {
        let _project = custom_agent_test_project(&mut app);
        app.is_remote = true;
        let mut remote = crate::tui::backend::RemoteConnection::dummy();
        let peer = remote.take_dummy_peer().unwrap();
        use tokio::io::{AsyncBufReadExt, BufReader};
        let mut reader = BufReader::new(peer);
        agent_wizard_generation_choice(&mut app);
        let before = app.display_messages.len();
        let mut line = String::new();
        super::remote::handle_remote_key(&mut app, KeyCode::Enter, KeyModifiers::NONE, &mut remote).await.unwrap();
        reader.read_line(&mut line).await.unwrap();
        let request: crate::protocol::Request = serde_json::from_str(&line).unwrap();
        let id = request.id();
        super::remote::handle_remote_key(&mut app, KeyCode::Esc, KeyModifiers::NONE, &mut remote).await.unwrap();
        line.clear();
        tokio::time::timeout(Duration::from_secs(2), reader.read_line(&mut line)).await.unwrap().unwrap();
        assert!(matches!(serde_json::from_str::<crate::protocol::Request>(&line).unwrap(), crate::protocol::Request::CancelAgentInstructions { generation_id, .. } if generation_id == id));
        let cancel_id = serde_json::from_str::<crate::protocol::Request>(&line).unwrap().id();
        for stale_id in [id, cancel_id] {
            app.handle_server_event(crate::protocol::ServerEvent::Error { id: stale_id, message: "Late canceled request error".into(), retry_after_secs: None }, &mut remote);
            assert_eq!(app.display_messages.len(), before, "canceled generation errors must remain private");
        }
        app.handle_server_event(crate::protocol::ServerEvent::AgentInstructionsGenerated { id, text: Some("Stale".into()), model: "model".into(), provider_name: "provider".into(), error: None }, &mut remote);
        assert!(app.agent_wizard.as_ref().unwrap().draft.prompt.is_empty());
        assert_eq!(app.agent_wizard.as_ref().unwrap().step, super::agent_wizard::Step::InstructionChoice);
        super::remote::handle_remote_key(&mut app, KeyCode::Enter, KeyModifiers::NONE, &mut remote).await.unwrap();
        line.clear();
        reader.read_line(&mut line).await.unwrap();
        let id = serde_json::from_str::<crate::protocol::Request>(&line).unwrap().id();
        app.handle_server_event(crate::protocol::ServerEvent::Error { id, message: "Unknown request: generate_agent_instructions".into(), retry_after_secs: None }, &mut remote);
        assert_eq!(app.agent_wizard.as_ref().unwrap().step, super::agent_wizard::Step::InstructionChoice);
        assert_eq!(app.display_messages.len(), before);
        assert!(!app.is_processing && !app.pending_turn);
    });
}

#[test]
fn agent_wizard_full_discard_emits_cancel_for_sent_generation_after_draft_is_gone() {
    let mut app = create_test_app();
    let rt = tokio::runtime::Runtime::new().unwrap();
    rt.block_on(async {
        let project = custom_agent_test_project(&mut app);
        app.is_remote = true;
        app.input = "Preserved composer".into();
        let before = app.display_messages.len();
        let mut remote = crate::tui::backend::RemoteConnection::dummy();
        let peer = remote.take_dummy_peer().unwrap();
        use tokio::io::{AsyncBufReadExt, BufReader};
        let mut reader = BufReader::new(peer);
        agent_wizard_generation_choice(&mut app);
        super::remote::handle_remote_key(&mut app, KeyCode::Enter, KeyModifiers::NONE, &mut remote).await.unwrap();
        let mut line = String::new();
        tokio::time::timeout(Duration::from_secs(2), reader.read_line(&mut line)).await.unwrap().unwrap();
        let id = match serde_json::from_str::<crate::protocol::Request>(&line).unwrap() {
            crate::protocol::Request::GenerateAgentInstructions { id, .. } => id,
            other => panic!("Expected sent generation, got {other:?}"),
        };
        app.handle_key(KeyCode::Char('c'), KeyModifiers::CONTROL).unwrap();
        agent_wizard_key(&mut app, KeyCode::Down);
        agent_wizard_key(&mut app, KeyCode::Enter);
        assert!(app.agent_wizard.is_none());
        assert_eq!(app.agent_wizard_remote_cancel, Some(id));
        app.dispatch_agent_wizard_generation(&mut remote).await;
        assert!(app.agent_wizard_remote_cancel.is_none());
        line.clear();
        tokio::time::timeout(Duration::from_secs(2), reader.read_line(&mut line)).await.unwrap().unwrap();
        assert!(matches!(serde_json::from_str::<crate::protocol::Request>(&line).unwrap(), crate::protocol::Request::CancelAgentInstructions { generation_id, .. } if generation_id == id));
        app.handle_server_event(crate::protocol::ServerEvent::AgentInstructionsGenerated { id, text: Some("Late text".into()), model: "model".into(), provider_name: "provider".into(), error: None }, &mut remote);
        app.handle_server_event(crate::protocol::ServerEvent::Error { id, message: "Late error".into(), retry_after_secs: None }, &mut remote);
        assert!(app.agent_wizard.is_none());
        assert!(!project.path().join(".jcode/agents/wizard-generated.md").exists());
        assert_eq!(app.input, "Preserved composer");
        assert_eq!(app.display_messages.len(), before);
        assert!(!app.pending_turn && !app.is_processing);
    });
}

#[test]
fn agent_wizard_remote_activation_failure_keeps_saved_file_and_previous_profile() {
    let rt = tokio::runtime::Runtime::new().unwrap();
    let _guard = rt.enter();
    let mut app = create_test_app();
    let project = custom_agent_test_project(&mut app);
    app.is_remote = true;
    app.remote_agent_profile_name = Some("previous".into());
    agent_wizard_manual_review(&mut app, "wizard-activate", "Instructions");
    agent_wizard_key(&mut app, KeyCode::Down);
    agent_wizard_key(&mut app, KeyCode::Enter);
    let path = project.path().join(".jcode/agents/wizard-activate.md");
    assert!(path.exists());
    assert_eq!(app.active_agent_profile_name(), Some("previous"));
    assert_eq!(
        app.pending_agent_profile,
        Some(Some("wizard-activate".into()))
    );
    app.pending_agent_profile = None;
    app.remote_agent_profile_request_id = Some(987);
    let mut remote = crate::tui::backend::RemoteConnection::dummy();
    app.handle_server_event(
        crate::protocol::ServerEvent::AgentProfileChanged {
            id: 987,
            name: Some("previous".into()),
            model: "old-model".into(),
            provider_name: None,
            effort: None,
            error: Some("Unsupported model".into()),
        },
        &mut remote,
    );
    assert!(path.exists());
    assert_eq!(app.active_agent_profile_name(), Some("previous"));
    assert!(
        app.agent_wizard_saved_path.is_none(),
        "authoritative result should finish the saved activation notice"
    );
    let notice = &app.status_notice.as_ref().unwrap().0;
    assert!(notice.contains(path.to_str().unwrap()));
    assert!(notice.contains("Activation failed: Unsupported model"));
}

#[test]
fn agent_wizard_activation_ack_refreshes_current_row_without_resetting_picker() {
    let rt = tokio::runtime::Runtime::new().unwrap();
    let _guard = rt.enter();
    let mut app = create_test_app();
    let project = custom_agent_test_project(&mut app);
    std::fs::write(
        project.path().join(".jcode/agents/previous.md"),
        "---\ndescription: Previous profile\n---\nPrevious instructions",
    )
    .unwrap();
    app.is_remote = true;
    app.remote_agent_profile_name = Some("previous".into());
    agent_wizard_manual_review(&mut app, "wizard-activate", "Instructions");
    agent_wizard_key(&mut app, KeyCode::Down);
    agent_wizard_key(&mut app, KeyCode::Enter);
    let picker = app.inline_interactive_state.as_mut().unwrap();
    let selected = picker.selected;
    let filtered = picker.filtered.clone();
    assert!(
        matches!(&picker.entries[picker.filtered[selected]].action, crate::tui::PickerAction::AgentProfile(Some(name)) if name == "wizard-activate")
    );
    picker.filter = "retained-filter".into();
    let chat_count = app.display_messages.len();
    std::fs::write(
        project.path().join(".jcode/agents/invalid.md"),
        "---\nname: [\n---\n",
    )
    .unwrap();
    app.pending_agent_profile = None;
    app.remote_agent_profile_request_id = Some(988);
    let mut remote = crate::tui::backend::RemoteConnection::dummy();
    app.handle_server_event(
        crate::protocol::ServerEvent::AgentProfileChanged {
            id: 988,
            name: Some("wizard-activate".into()),
            model: "current-model".into(),
            provider_name: None,
            effort: None,
            error: None,
        },
        &mut remote,
    );
    let picker = app.inline_interactive_state.as_ref().unwrap();
    assert_eq!(picker.selected, selected);
    assert_eq!(picker.filtered, filtered);
    assert_eq!(picker.filter, "retained-filter");
    assert_eq!(
        picker
            .entries
            .iter()
            .filter(|entry| entry.is_current)
            .count(),
        1
    );
    assert!(
        picker
            .entries
            .iter()
            .any(|entry| entry.name == "wizard-activate" && entry.is_current)
    );
    assert!(
        picker
            .entries
            .iter()
            .any(|entry| entry.name == "previous" && !entry.is_current)
    );
    assert_eq!(
        app.display_messages.len(),
        chat_count + 1,
        "only authoritative activation confirmation belongs in chat"
    );
    assert!(app.agent_wizard_saved_path.is_none());
}

#[test]
fn agent_wizard_remote_generation_response_fills_editable_body_not_chat() {
    let mut app = create_test_app();
    let rt = tokio::runtime::Runtime::new().unwrap();
    rt.block_on(async {
        let _project = custom_agent_test_project(&mut app);
        app.is_remote = true;
        app.input = "Original composer".into();
        app.cursor_pos = 3;
        let before = app.display_messages.len();
        let mut remote = crate::tui::backend::RemoteConnection::dummy();
        let peer = remote.take_dummy_peer().unwrap();
        agent_wizard_generation_choice(&mut app);
        super::remote::handle_remote_key(&mut app, KeyCode::Enter, KeyModifiers::NONE, &mut remote)
            .await
            .unwrap();
        use tokio::io::{AsyncBufReadExt, BufReader};
        let mut reader = BufReader::new(peer);
        let mut line = String::new();
        tokio::time::timeout(Duration::from_secs(2), reader.read_line(&mut line))
            .await
            .unwrap()
            .unwrap();
        let request: crate::protocol::Request = serde_json::from_str(&line).unwrap();
        let id = match request {
            crate::protocol::Request::GenerateAgentInstructions {
                id,
                description,
                purpose,
                mode,
            } => {
                assert_eq!(description, "Review code");
                assert_eq!(purpose, "Find concrete correctness errors");
                assert_eq!(mode, "all");
                id
            }
            other => panic!("Expected dedicated generation, got {other:?}"),
        };
        let mut terminal =
            ratatui::Terminal::new(ratatui::backend::TestBackend::new(28, 6)).unwrap();
        terminal
            .draw(|frame| crate::tui::ui::draw(frame, &app))
            .unwrap();
        let text = terminal
            .backend()
            .buffer()
            .content()
            .iter()
            .map(|cell| cell.symbol())
            .collect::<String>();
        assert!(text.contains("Esc cancel"));
        assert!(!text.contains("Esc back"));
        app.handle_server_event(
            crate::protocol::ServerEvent::AgentInstructionsGenerated {
                id,
                text: Some("Generated instructions".into()),
                model: "generator-model".into(),
                provider_name: "generator-provider".into(),
                error: None,
            },
            &mut remote,
        );
        assert_eq!(
            app.agent_wizard.as_ref().unwrap().step,
            super::agent_wizard::Step::Instructions
        );
        assert_eq!(
            app.agent_wizard.as_ref().unwrap().draft.prompt,
            "Generated instructions"
        );
        assert_eq!(
            app.agent_wizard.as_ref().unwrap().draft.name,
            "wizard-generated"
        );
        assert_eq!(app.display_messages.len(), before);
        assert_eq!(app.input, "Original composer");
        assert!(!app.pending_turn && !app.is_processing);
        app.handle_paste(" edited".into());
        agent_wizard_key(&mut app, KeyCode::Enter);
        assert_eq!(
            app.agent_wizard.as_ref().unwrap().draft.prompt,
            "Generated instructions edited"
        );
    });
}
