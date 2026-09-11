use super::*;
use anyhow::{Result, anyhow};

fn parse_request_json(json: &str) -> Result<Request> {
    serde_json::from_str(json).map_err(Into::into)
}

fn parse_event_json(json: &str) -> Result<ServerEvent> {
    serde_json::from_str(json).map_err(Into::into)
}

include!("protocol_tests/core_events.rs");
include!("protocol_tests/comm_requests.rs");
include!("protocol_tests/comm_responses.rs");
include!("protocol_tests/comm_format_awaited.rs");
include!("protocol_tests/misc_events.rs");
include!("protocol_tests/randomized.rs");

#[test]
fn rename_account_request_roundtrips() {
    let input = r#"{"type":"rename_account","id":42,"provider":"openai","label":"old name","new_label":"Work / name"}"#;
    let request: Request = serde_json::from_str(input).unwrap();
    assert_eq!(request.id(), 42);
    let value = serde_json::to_value(request).unwrap();
    assert_eq!(value["new_label"], "Work / name");
}
