#![cfg(not(feature = "telemetry"))]

use std::net::TcpListener;
use std::process::Command;
use std::time::Duration;

#[test]
fn build_policy_blocks_enablement_and_reporting() {
    const CHILD: &str = "JCODE_TEST_BUILD_POLICY_CHILD";
    if std::env::var_os(CHILD).is_none() {
        let home = tempfile::tempdir().unwrap();
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let status = Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "build_policy_blocks_enablement_and_reporting",
                "--nocapture",
            ])
            .env(CHILD, "1")
            .env("JCODE_HOME", home.path())
            .env(
                "JCODE_TELEMETRY_BASE_URL",
                format!("http://{}", listener.local_addr().unwrap()),
            )
            .env("JCODE_NO_TELEMETRY", "0")
            .env("DO_NOT_TRACK", "0")
            .status()
            .unwrap();
        assert!(status.success());
        assert_eq!(
            listener.accept().unwrap_err().kind(),
            std::io::ErrorKind::WouldBlock
        );
        assert!(!home.path().join("telemetry_id").exists());
        return;
    }

    use jcode_telemetry_core as telemetry;
    let home = std::path::PathBuf::from(std::env::var_os("JCODE_HOME").unwrap());
    std::fs::write(home.join("telemetry_share_transcripts_v1"), "1").unwrap();
    std::fs::write(home.join("config.toml"), "[telemetry]\nenabled = true\n").unwrap();
    assert!(!telemetry::telemetry_compiled_in());
    assert!(!telemetry::set_usage_telemetry_enabled(true));
    assert!(!telemetry::set_content_sharing_enabled(true));
    telemetry::set_category_settings([true; 6]);
    assert_eq!(telemetry::category_settings(), [false; 6]);
    assert!(!telemetry::is_enabled());
    assert!(!telemetry::content_sharing_enabled());
    assert_eq!(
        telemetry::status().opt_out_source,
        Some(telemetry::TelemetryOptOutSource::BuildPolicy)
    );
    telemetry::record_turn();
    telemetry::record_assistant_response();
    telemetry::record_install_if_first_run();
    telemetry::record_upgrade_if_needed();
    telemetry::record_feedback("build-policy regression");
    telemetry::record_auth_success("openai", "oauth");
    assert!(!telemetry::enqueue_oauth_account(
        "openai",
        "test@example.invalid"
    ));
    assert!(!telemetry::record_transcript(
        "openai",
        "test",
        telemetry::SessionEndReason::NormalExit,
        serde_json::json!([{"role": "user", "content": "build-policy regression"}])
    ));
    assert!(telemetry::set_usage_telemetry_enabled(false));
    telemetry::flush_pending(Duration::from_millis(100));
    assert!(telemetry::status().telemetry_id.is_none());
}
