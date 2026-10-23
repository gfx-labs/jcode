use super::*;

fn valid_options() -> Map<String, Value> {
    Map::from_iter([
        (
            "option_a".into(),
            json!("Review the change with a large-context model"),
        ),
        (
            "option_b".into(),
            json!("Implement the change with a fast model"),
        ),
    ])
}

fn test_ctx() -> ToolContext {
    ToolContext {
        session_id: "test-session".into(),
        message_id: "test-message".into(),
        tool_call_id: "test-call".into(),
        working_dir: None,
        stdin_request_tx: None,
        graceful_shutdown_signal: None,
        execution_mode: crate::tool::ToolExecutionMode::Direct,
    }
}

#[test]
fn parse_choice_accepts_offered_ids_and_rejects_unknown_ids() {
    let options = valid_options();
    let good = json!({"answers": {"decision": {"type": "choice", "choice": "option_b", "confidence": 0.8}}});
    let (choice, confidence) = parse_choice(good, &options).unwrap();
    assert_eq!(choice, "option_b");
    assert!((confidence.expect("reported confidence") - 0.8).abs() < f32::EPSILON);
    let bad = json!({"answers": {"decision": {"type": "choice", "choice": "evil_option", "confidence": 0.9}}});
    let error = parse_choice(bad, &options).unwrap_err().to_string();
    assert!(
        error.contains("not one of the offered options"),
        "error should name the rejection clearly: {error}"
    );
}

#[tokio::test]
async fn execute_rejects_bad_input_before_any_network_call() {
    let tool = JevDecisionTool::new();
    // One option: below the upstream minimum.
    let one_option = json!({
        "instructions": "Pick.",
        "options": {"only": "A"}
    });
    let err = tool
        .execute(one_option, test_ctx())
        .await
        .unwrap_err()
        .to_string();
    assert!(err.contains("2 to 255"), "{err}");
    // Empty instructions.
    let no_instructions = json!({"instructions": "   ", "options": valid_options()});
    let err = tool
        .execute(no_instructions, test_ctx())
        .await
        .unwrap_err()
        .to_string();
    assert!(err.contains("instructions"), "{err}");
    // Non-object state.
    let bad_state = json!({
        "instructions": "Pick.",
        "state": "just a string",
        "options": valid_options()
    });
    let err = tool
        .execute(bad_state, test_ctx())
        .await
        .unwrap_err()
        .to_string();
    assert!(err.contains("state"), "{err}");
    // Empty option description.
    let blank = json!({
        "instructions": "Pick.",
        "options": {"a": "A", "b": "  "}
    });
    let err = tool
        .execute(blank, test_ctx())
        .await
        .unwrap_err()
        .to_string();
    assert!(err.contains("non-empty description"), "{err}");
}
