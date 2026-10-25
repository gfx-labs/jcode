#[test]
fn compact_controls_expand_only_tools_and_preserve_thinking() {
    with_temp_jcode_home(|| {
        let mut app = create_test_app();
        let tool = DisplayMessage::tool(
            "tool output".to_string(),
            crate::message::ToolCall {
                id: "compact_controls_tool".to_string(),
                name: "bash".to_string(),
                input: serde_json::json!({"command": "echo output"}),
                intent: None,
                thought_signature: None,
            },
        );
        let tool_hash = tool.stable_cache_hash();
        let reasoning =
            DisplayMessage::reasoning(jcode_tui_markdown::reasoning_line_markup("Later thinking"));
        let reasoning_hash = reasoning.stable_cache_hash();
        let assistant = DisplayMessage::assistant(format!(
            "{}\nAnswer.",
            jcode_tui_markdown::reasoning_line_markup("Latest thinking")
        ));
        let assistant_hash = assistant.stable_cache_hash();
        app.display_messages = vec![tool, reasoning, assistant];
        for hash in [tool_hash, reasoning_hash, assistant_hash] {
            jcode_tui_messages::set_transcript_message_expanded(hash, false);
        }
        assert!(commands::handle_config_command(
            &mut app,
            "/compact-transcript expand"
        ));
        assert!(jcode_tui_messages::transcript_message_expanded(tool_hash));
        assert!(!jcode_tui_messages::transcript_message_expanded(
            reasoning_hash
        ));
        assert!(!jcode_tui_messages::transcript_message_expanded(
            assistant_hash
        ));
        assert!(commands::handle_config_command(
            &mut app,
            "/compact-transcript expand"
        ));
        assert!(!jcode_tui_messages::transcript_message_expanded(tool_hash));

        // With no tool result, this command must not fall back to thinking.
        app.display_messages.remove(0);
        assert!(commands::handle_config_command(
            &mut app,
            "/compact-transcript expand"
        ));
        assert_eq!(
            app.status_notice.as_ref().map(|(text, _)| text.as_str()),
            Some("No tool result to expand")
        );
        assert!(!jcode_tui_messages::transcript_message_expanded(
            assistant_hash
        ));
    });
}

#[test]
fn compact_controls_help_describes_independent_thinking() {
    with_temp_jcode_home(|| {
        let mut app = create_test_app();
        assert!(commands::handle_config_command(
            &mut app,
            "/compact-transcript"
        ));
        let help = &app.display_messages.last().unwrap().content;
        assert!(help.contains("Thinking is controlled independently by /thinking-display"));
        assert!(!help.contains("every tool result and thinking"));
    });
}
#[test]
fn compact_links_persist_and_labels_open_from_real_wrapped_frames() {
    with_temp_jcode_home(|| {
        let _render = scroll_render_test_lock();
        crate::tui::markdown::install_jcode_markdown_hooks();
        let mut app = create_test_app();
        assert!(!crate::config::config().display.compact_links);
        assert!(
            app.command_help("compact-links")
                .unwrap()
                .contains("/compact-links on")
        );
        let suggestions = app.get_suggestions_for("/compact-links ");
        assert!(
            suggestions
                .iter()
                .any(|(text, _)| text == "/compact-links on")
        );
        let markdown = "[**α界β界γ**](https://example.test/a_(b)) and [δ界ε界ζ](docs/two.md#part)";
        for (command, compact) in [("on", true), ("off", false), ("toggle", true)] {
            app.input = format!("/compact-links {command}");
            app.submit_input();
            assert_eq!(crate::config::Config::load().display.compact_links, compact);
            for centered in [false, true] {
                app.centered = centered;
                for width in [18, 32, 80] {
                    for content in [
                        markdown.to_string(),
                        format!("> {markdown}"),
                        format!("- {markdown}"),
                        format!("| links |\n| -- |\n| {markdown} |"),
                        format!("```plan\n# Plan\n{markdown}\n```"),
                    ] {
                        app.display_messages = vec![
                            DisplayMessage::user("Read links"),
                            DisplayMessage::assistant(content.clone()),
                        ];
                        app.bump_display_messages_version();
                        assert_compact_link_frame(
                            &app,
                            width,
                            width == 80,
                            &format!("{command} {centered} {content}"),
                        );
                    }
                }
            }
        }
        let repository = tempfile::tempdir().unwrap();
        std::fs::write(repository.path().join("links.md"), markdown).unwrap();
        app.session.working_dir = Some(repository.path().to_string_lossy().into_owned());
        app.display_messages = vec![DisplayMessage::user("Read links")];
        app.replace_streaming_text(markdown.to_string());
        app.is_processing = true;
        app.bump_display_messages_version();
        for command in ["off", "on"] {
            assert!(commands::handle_compact_links_command(
                &mut app,
                &format!("/compact-links {command}")
            ));
            for centered in [false, true] {
                app.centered = centered;
                for width in [18, 80] {
                    assert_compact_link_frame(
                        &app,
                        width,
                        true,
                        &format!("streaming {command} {centered}"),
                    );
                }
            }
        }
        app.clear_streaming_render_state();
        app.is_processing = false;
        assert!(app.try_open_repository_markdown_link("links.md"));
        for command in ["off", "on"] {
            assert!(commands::handle_compact_links_command(
                &mut app,
                &format!("/compact-links {command}")
            ));
            for centered in [false, true] {
                app.centered = centered;
                assert_compact_link_frame(
                    &app,
                    160,
                    true,
                    &format!("side pane {command} {centered}"),
                );
            }
        }
        app.input = "/compact-links invalid".into();
        app.submit_input();
        assert!(
            app.display_messages
                .last()
                .unwrap()
                .content
                .contains("Usage: /compact-links")
        );
        assert!(crate::config::Config::load().display.compact_links);
    });
}

fn assert_compact_link_frame(app: &App, width: u16, all_labels: bool, context: &str) {
    let mut terminal =
        ratatui::Terminal::new(ratatui::backend::TestBackend::new(width, 80)).unwrap();
    let screen = render_and_snap(app, &mut terminal);
    let buffer = terminal.backend().buffer();
    let mut found = 0;
    for row in 0..buffer.area().height {
        for column in 0..buffer.area().width {
            let expected = match buffer[(column, row)].symbol() {
                "α" | "β" | "γ" => "https://example.test/a_(b)",
                "δ" | "ε" | "ζ" => "docs/two.md#part",
                _ => continue,
            };
            assert_eq!(
                crate::tui::ui::link_target_from_screen(column, row).as_deref(),
                Some(expected),
                "{context} width={width} at ({column},{row}):\n{screen}"
            );
            found += 1;
        }
    }
    assert!(found > 0, "no visible labels: {context}\n{screen}");
    if all_labels {
        assert_eq!(found, 6, "labels missing: {context}\n{screen}");
    }
}
