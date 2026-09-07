//! Run the real CLI. Status must never create an identity or send telemetry.
use std::process::Command;

fn status(base: Option<&str>, json: bool) -> (tempfile::TempDir, String) {
    let home = tempfile::tempdir().unwrap();
    let mut command = Command::new(env!("CARGO_BIN_EXE_jcode"));
    command
        .args(["--no-update", "telemetry", "status"])
        .env("JCODE_HOME", home.path())
        .env("JCODE_NO_TELEMETRY", "1")
        .env_remove("DO_NOT_TRACK")
        .env_remove("JCODE_TELEMETRY_BASE_URL");
    if let Some(base) = base {
        command.env("JCODE_TELEMETRY_BASE_URL", base);
    }
    if json {
        command.arg("--json");
    }
    let output = command.output().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(!home.path().join("telemetry_id").exists());
    (home, String::from_utf8(output.stdout).unwrap())
}

#[test]
fn telemetry_status_shows_default_destinations_without_creating_identity() {
    let (_home, output) = status(None, true);
    let value: serde_json::Value = serde_json::from_str(&output).unwrap();
    assert_eq!(
        value["event_endpoint"],
        "https://telemetry.jcode.sh/v1/event"
    );
    assert_eq!(
        value["transcript_endpoint"],
        "https://telemetry.jcode.sh/v1/transcript"
    );
    assert!(value["telemetry_id"].is_null());
    assert!(value["endpoint_error"].is_null());
}

#[test]
fn telemetry_status_shows_both_local_destinations_and_preserves_opt_out() {
    let (_home, output) = status(Some("http://127.0.0.1:8080/prefix/"), true);
    let value: serde_json::Value = serde_json::from_str(&output).unwrap();
    assert_eq!(
        value["event_endpoint"],
        "http://127.0.0.1:8080/prefix/v1/event"
    );
    assert_eq!(
        value["transcript_endpoint"],
        "http://127.0.0.1:8080/prefix/v1/transcript"
    );
    assert_eq!(value["enabled"], false);
    assert_eq!(value["content_sharing_enabled"], false);
    assert_eq!(value["opt_out_source"], "environment");
    let (_home, text) = status(Some("http://127.0.0.1:8080"), false);
    assert!(text.contains("Event endpoint: http://127.0.0.1:8080/v1/event"));
    assert!(text.contains("Transcript endpoint: http://127.0.0.1:8080/v1/transcript"));
}

#[test]
fn invalid_override_is_visible_without_exposing_credentials_or_public_fallback() {
    for base in ["", "not-a-url", "http://user:secret@localhost"] {
        let (_home, output) = status(Some(base), true);
        let value: serde_json::Value = serde_json::from_str(&output).unwrap();
        assert!(value["event_endpoint"].is_null());
        assert!(value["transcript_endpoint"].is_null());
        assert!(value["endpoint_error"].is_string());
        assert!(!output.contains("secret"));
        let (_home, text) = status(Some(base), false);
        assert!(text.contains("Reporting destination: invalid"));
        assert!(text.contains("delivery disabled"));
        assert!(!text.contains("telemetry.jcode.sh"));
        assert!(!text.contains("secret"));
    }
}
