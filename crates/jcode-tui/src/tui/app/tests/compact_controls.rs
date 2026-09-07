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
