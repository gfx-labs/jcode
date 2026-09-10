fn custom_agent_test_project(app: &mut App) -> tempfile::TempDir {
    let project = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(project.path().join(".jcode/agents")).unwrap();
    app.session.working_dir = Some(project.path().to_string_lossy().into_owned());
    project
}

#[test]
fn custom_agents_picker_reloads_profiles_and_preserves_services() {
    let mut app = create_test_app();
    let project = custom_agent_test_project(&mut app);
    let profile = project.path().join(".jcode/agents/tui-test-designer.md");
    std::fs::write(
        &profile,
        "---\ndescription: Design interfaces\nmode: primary\n---\nUse clear labels.",
    )
    .unwrap();
    app.open_agents_picker();
    let picker = app.inline_interactive_state.as_ref().unwrap();
    assert!(picker.is_agent_target_picker());
    assert!(
        picker
            .entries
            .iter()
            .any(|entry| entry.name == "tui-test-designer")
    );
    assert_eq!(
        picker
            .entries
            .iter()
            .filter(|entry| matches!(entry.action, crate::tui::PickerAction::AgentTarget(_)))
            .count(),
        5
    );
    assert_eq!(picker.filtered.len(), picker.entries.len());

    std::fs::write(
        &profile,
        "---\ndescription: Review interfaces\nmode: subagent\n---\nReview labels.",
    )
    .unwrap();
    app.open_agents_picker();
    let picker = app.inline_interactive_state.as_ref().unwrap();
    let entry = picker
        .entries
        .iter()
        .find(|entry| entry.name == "tui-test-designer")
        .unwrap();
    assert!(!entry.options[0].available);
    assert!(entry.options[0].detail.contains("subagent-only"));
    assert!(picker.filter_text(entry).contains("Review interfaces"));
}

#[test]
fn custom_agents_discovery_errors_are_visible() {
    let mut app = create_test_app();
    let project = custom_agent_test_project(&mut app);
    std::fs::write(
        project.path().join(".jcode/agents/tui-test-invalid.md"),
        "---\npermissions: allow\n---\nUnsafe expectations.",
    )
    .unwrap();
    app.open_agents_picker();
    assert!(
        app.display_messages
            .iter()
            .any(|message| message.content.contains("tui-test-invalid")
                && message.content.contains("permissions"))
    );
}

#[test]
fn custom_agents_help_and_completion_explain_session_scope() {
    let mut app = create_test_app();
    let project = custom_agent_test_project(&mut app);
    std::fs::write(
        project.path().join(".jcode/agents/tui-test-designer.md"),
        "---\ndescription: Design interfaces\n---\nUse clear labels.",
    )
    .unwrap();
    assert!(
        app.get_suggestions_for("/agents tui-test")
            .iter()
            .any(|(command, _)| command == "/agents tui-test-designer")
    );
    assert!(
        app.get_suggestions_for("/agents use tui-test")
            .iter()
            .any(|(command, _)| command == "/agents use tui-test-designer")
    );
    assert!(
        app.get_suggestions_for("/agents cl")
            .iter()
            .any(|(command, _)| command == "/agents clear")
    );
    let help = app.command_help("agents").unwrap();
    assert!(help.contains("/agents use <name>"));
    assert!(help.contains("/agents clear"));
    assert!(help.contains("current session"));
    assert!(help.contains("subagent-only"));
}

#[test]
fn custom_agents_command_does_not_capture_unrelated_prefix() {
    let mut app = create_test_app();
    assert!(!super::commands::handle_agents_command(
        &mut app,
        "/agentsmith"
    ));
}

#[test]
fn custom_agents_local_activation_preserves_transcript_and_clear_preserves_model() {
    let mut app = create_test_app();
    let project = custom_agent_test_project(&mut app);
    std::fs::write(project.path().join(".jcode/agents/tui-test-designer.md"), "---\ndescription: Design interfaces\nmode: all\n---\nProfile fixture: use accessible interface labels.").unwrap();
    app.push_display_message(DisplayMessage::system("Existing conversation".to_string()));
    let model = app.provider.model();
    assert!(super::commands::handle_agents_command(
        &mut app,
        "/agents tui-test-designer"
    ));
    assert!(!app.is_processing);
    assert!(!app.pending_turn);
    assert!(
        app.display_messages
            .iter()
            .any(|message| message.content == "Existing conversation")
    );
    assert!(
        app.build_system_prompt_split(None)
            .static_part
            .contains("Profile fixture: use accessible interface labels.")
    );
    assert!(super::commands::handle_agents_command(
        &mut app,
        "/agents clear"
    ));
    assert_eq!(app.provider.model(), model);
    assert!(
        !app.build_system_prompt_split(None)
            .static_part
            .contains("Profile fixture: use accessible interface labels.")
    );
}

#[test]
fn custom_agents_reject_primary_activation_of_subagent_only_profile() {
    let mut app = create_test_app();
    let project = custom_agent_test_project(&mut app);
    std::fs::write(
        project.path().join(".jcode/agents/tui-test-worker.md"),
        "---\ndescription: Worker\nmode: subagent\n---\nWorker fixture instructions.",
    )
    .unwrap();
    super::commands::handle_agents_command(&mut app, "/agents tui-test-worker");
    assert!(
        app.display_messages
            .iter()
            .any(|message| message.content.contains("subagent"))
    );
    assert!(
        !app.build_system_prompt_split(None)
            .static_part
            .contains("Worker fixture instructions.")
    );
    assert!(!app.pending_turn);
}

#[test]
fn custom_agents_reject_changes_during_processing() {
    let mut app = create_test_app();
    app.is_processing = true;
    super::commands::handle_agents_command(&mut app, "/agents clear");
    assert!(
        app.display_messages
            .iter()
            .any(|message| message.content.contains("processing"))
    );
    assert!(!app.pending_turn);
}

#[test]
fn custom_agents_builtin_collision_uses_explicit_use_command() {
    let mut app = create_test_app();
    let project = custom_agent_test_project(&mut app);
    std::fs::write(
        project.path().join(".jcode/agents/review.md"),
        "---\ndescription: Custom review\n---\nCustom review fixture instructions.",
    )
    .unwrap();
    configure_test_remote_models(&mut app);
    super::commands::handle_agents_command(&mut app, "/agents review");
    wait_for_model_picker_load(&mut app);
    assert!(
        app.inline_interactive_state
            .as_ref()
            .unwrap()
            .entries
            .iter()
            .all(|entry| matches!(
                entry.action,
                crate::tui::PickerAction::AgentModelChoice {
                    target: crate::tui::AgentModelTarget::Review,
                    ..
                }
            ))
    );
    app.is_remote = false;
    super::commands::handle_agents_command(&mut app, "/agents use review");
    assert!(
        app.build_system_prompt_split(None)
            .static_part
            .contains("Custom review fixture instructions.")
    );
}

#[test]
fn custom_agents_remote_selection_waits_for_authoritative_confirmation() {
    let mut app = create_test_app();
    app.is_remote = true;
    app.remote_agent_profile_name = Some("previous".to_string());
    app.remote_provider_model = Some("previous-model".to_string());
    super::commands::handle_agents_command(&mut app, "/agents server-only-profile");
    assert_eq!(
        app.pending_agent_profile,
        Some(Some("server-only-profile".to_string()))
    );
    assert_eq!(app.active_agent_profile_name(), Some("previous"));
    assert!(!app.pending_turn);
    assert!(!app.is_processing);
    assert!(
        !app.display_messages
            .iter()
            .any(|message| message.content.contains("active for the current session"))
    );

    app.pending_agent_profile = None;
    app.remote_agent_profile_request_id = Some(42);
    let rt = tokio::runtime::Runtime::new().unwrap();
    let _guard = rt.enter();
    let mut remote = crate::tui::backend::RemoteConnection::dummy();
    app.handle_server_event(
        crate::protocol::ServerEvent::AgentProfileChanged {
            provider_name: Some("fixture-provider".to_string()),
            id: 42,
            name: Some("server-only-profile".to_string()),
            model: "server-model".to_string(),
            effort: Some("high".to_string()),
            error: None,
        },
        &mut remote,
    );
    assert_eq!(app.active_agent_profile_name(), Some("server-only-profile"));
    assert_eq!(app.remote_provider_model.as_deref(), Some("server-model"));
    assert_eq!(
        app.remote_provider_name.as_deref(),
        Some("fixture-provider")
    );
    assert_eq!(app.remote_reasoning_effort.as_deref(), Some("high"));
    assert!(!app.agent_profile_switch_pending());
    assert!(!app.pending_turn);
}

#[test]
fn custom_agents_remote_failure_keeps_profile_and_model_then_clear_confirms() {
    let mut app = create_test_app();
    app.is_remote = true;
    app.remote_agent_profile_name = Some("previous".to_string());
    app.remote_provider_model = Some("previous-model".to_string());
    app.remote_agent_profile_request_id = Some(42);
    let rt = tokio::runtime::Runtime::new().unwrap();
    let _guard = rt.enter();
    let mut remote = crate::tui::backend::RemoteConnection::dummy();
    app.handle_server_event(
        crate::protocol::ServerEvent::AgentProfileChanged {
            provider_name: Some("fixture-provider".to_string()),
            id: 42,
            name: Some("bad-profile".to_string()),
            model: "bad-model".to_string(),
            effort: None,
            error: Some("Missing skill: fixture".to_string()),
        },
        &mut remote,
    );
    assert_eq!(app.active_agent_profile_name(), Some("previous"));
    assert_eq!(app.remote_provider_model.as_deref(), Some("previous-model"));
    assert!(!app.agent_profile_switch_pending());
    assert!(
        app.display_messages
            .last()
            .unwrap()
            .content
            .contains("Missing skill: fixture")
    );

    super::commands::handle_agents_command(&mut app, "/agents clear");
    assert_eq!(app.pending_agent_profile, Some(None));
    assert_eq!(app.active_agent_profile_name(), Some("previous"));
    app.pending_agent_profile = None;
    app.remote_agent_profile_request_id = Some(43);
    app.handle_server_event(
        crate::protocol::ServerEvent::AgentProfileChanged {
            provider_name: Some("fixture-provider".to_string()),
            id: 43,
            name: None,
            model: "previous-model".to_string(),
            effort: None,
            error: None,
        },
        &mut remote,
    );
    assert_eq!(app.active_agent_profile_name(), None);
    assert_eq!(app.remote_provider_model.as_deref(), Some("previous-model"));
    assert!(
        app.display_messages
            .last()
            .unwrap()
            .content
            .contains("model unchanged")
    );
}

#[test]
fn custom_agents_remote_attach_sync_is_silent() {
    let mut app = create_test_app();
    app.is_remote = true;
    let count = app.display_messages.len();
    let rt = tokio::runtime::Runtime::new().unwrap();
    let _guard = rt.enter();
    let mut remote = crate::tui::backend::RemoteConnection::dummy();
    app.handle_server_event(
        crate::protocol::ServerEvent::AgentProfileChanged {
            provider_name: Some("fixture-provider".to_string()),
            id: 0,
            name: Some("restored".to_string()),
            model: "restored-model".to_string(),
            effort: None,
            error: None,
        },
        &mut remote,
    );
    assert_eq!(app.active_agent_profile_name(), Some("restored"));
    assert_eq!(app.display_messages.len(), count);
}

#[test]
fn custom_agents_remote_forwards_selection_and_clear_without_starting_turn() {
    let mut app = create_test_app();
    app.is_remote = true;
    let rt = tokio::runtime::Runtime::new().unwrap();
    rt.block_on(async {
        use tokio::io::AsyncBufReadExt;
        let mut remote = crate::tui::backend::RemoteConnection::dummy();
        let mut peer = tokio::io::BufReader::new(remote.take_dummy_peer().unwrap());
        for name in [Some("fixture"), None] {
            app.pending_agent_profile = Some(name.map(str::to_owned));
            super::remote::process_remote_followups(&mut app, &mut remote).await;
            let mut line = String::new();
            tokio::time::timeout(std::time::Duration::from_secs(1), peer.read_line(&mut line))
                .await
                .expect("profile request should be sent")
                .unwrap();
            let request: crate::protocol::Request = serde_json::from_str(&line).unwrap();
            match request {
                crate::protocol::Request::SetAgentProfile { id, name: sent } => {
                    assert_eq!(sent.as_deref(), name);
                    assert_eq!(app.remote_agent_profile_request_id, Some(id));
                }
                other => panic!("expected profile request, got {other:?}"),
            }
            assert!(!app.is_processing);
            assert!(!app.pending_turn);
            assert!(app.pending_agent_profile.is_none());
            app.remote_agent_profile_request_id = None;
        }
    });
}

#[test]
fn custom_agents_remote_defers_prompt_until_profile_is_confirmed() {
    let mut app = create_test_app();
    app.is_remote = true;
    app.remote_agent_profile_request_id = Some(42);
    let rt = tokio::runtime::Runtime::new().unwrap();
    rt.block_on(async {
        let mut remote = crate::tui::backend::RemoteConnection::dummy();
        remote.mark_history_loaded();
        super::remote::submit_prepared_remote_input(
            &mut app,
            &mut remote,
            super::input::PreparedInput {
                raw_input: "Use the selected profile".to_string(),
                expanded: "Use the selected profile".to_string(),
                images: Vec::new(),
            },
        )
        .await
        .unwrap();
        assert!(app.pending_prompt_after_model_switch.is_some());
        assert!(!app.is_processing);
        assert_eq!(remote.next_request_id_for_test(), 1);
    });
}

#[test]
fn custom_agents_picker_keyboard_selection_and_escape_do_not_start_turns() {
    let mut app = create_test_app();
    app.is_remote = true;
    let project = custom_agent_test_project(&mut app);
    for (name, mode) in [
        ("tui-test-primary", "primary"),
        ("tui-test-worker", "subagent"),
    ] {
        std::fs::write(
            project.path().join(format!(".jcode/agents/{name}.md")),
            format!("---\ndescription: Keyboard fixture\nmode: {mode}\n---\nFixture instructions."),
        )
        .unwrap();
    }
    for (name, key, expected) in [
        ("tui-test-primary", crossterm::event::KeyCode::Esc, None),
        ("tui-test-worker", crossterm::event::KeyCode::Enter, None),
        (
            "tui-test-primary",
            crossterm::event::KeyCode::Enter,
            Some(Some("tui-test-primary".to_string())),
        ),
    ] {
        app.open_agents_picker();
        let picker = app.inline_interactive_state.as_mut().unwrap();
        picker.selected = picker
            .entries
            .iter()
            .position(|entry| entry.name == name)
            .unwrap();
        app.handle_inline_interactive_key(key, crossterm::event::KeyModifiers::NONE)
            .unwrap();
        assert_eq!(app.pending_agent_profile, expected);
        assert!(app.inline_interactive_state.is_none());
        assert!(!app.pending_turn);
        assert!(!app.is_processing);
    }
}

#[test]
fn custom_agents_attach_snapshot_does_not_acknowledge_an_inflight_change() {
    let mut app = create_test_app();
    app.is_remote = true;
    app.remote_agent_profile_request_id = Some(42);
    let rt = tokio::runtime::Runtime::new().unwrap();
    let _guard = rt.enter();
    let mut remote = crate::tui::backend::RemoteConnection::dummy();
    app.handle_server_event(
        crate::protocol::ServerEvent::AgentProfileChanged {
            provider_name: Some("fixture-provider".to_string()),
            id: 0,
            name: None,
            model: "startup-model".to_string(),
            effort: None,
            error: None,
        },
        &mut remote,
    );
    assert_eq!(app.remote_agent_profile_request_id, Some(42));
}

#[test]
fn custom_agents_remote_send_failure_restores_queued_prompt_and_keeps_profile() {
    let mut app = create_test_app();
    app.is_remote = true;
    app.remote_agent_profile_name = Some("previous".to_string());
    app.pending_agent_profile = Some(Some("fixture".to_string()));
    app.pending_prompt_after_model_switch = Some(super::input::PreparedInput {
        raw_input: "Queued fixture".to_string(),
        expanded: "Queued fixture".to_string(),
        images: vec![],
    });
    let rt = tokio::runtime::Runtime::new().unwrap();
    rt.block_on(async {
        let mut remote = crate::tui::backend::RemoteConnection::dummy();
        drop(remote.take_dummy_peer());
        super::remote::process_remote_followups(&mut app, &mut remote).await;
    });
    assert_eq!(app.input, "Queued fixture");
    assert_eq!(app.active_agent_profile_name(), Some("previous"));
    assert!(!app.agent_profile_switch_pending());
    assert!(!app.is_processing);
    assert!(app.display_messages.iter().any(|message| {
        message
            .content
            .contains("Failed to request agent profile change")
    }));
}
