//! Exercise pairing through the real CLI without touching the user's registry
//! or starting a listener. Each invocation has a disposable config directory.
use std::process::Command;

fn pair(bind: &str, configured_host: Option<&str>, env_host: Option<&str>) -> String {
    let home = tempfile::tempdir().unwrap();
    let mut config = format!("[gateway]\nenabled = true\nport = 7643\nbind_addr = {bind:?}\n");
    if let Some(host) = configured_host {
        config.push_str(&format!("connect_host = {host:?}\n"));
    }
    std::fs::write(home.path().join("config.toml"), &config).unwrap();
    let mut command = Command::new(env!("CARGO_BIN_EXE_jcode"));
    command
        .args(["--no-update", "pair"])
        .current_dir(home.path())
        .env("JCODE_HOME", home.path())
        .env("JCODE_NO_TELEMETRY", "1")
        .env_remove("JCODE_GATEWAY_ENABLED")
        .env_remove("JCODE_GATEWAY_BIND_ADDR")
        .env_remove("JCODE_GATEWAY_PORT")
        .env_remove("JCODE_GATEWAY_HOST");
    if let Some(host) = env_host {
        command.env("JCODE_GATEWAY_HOST", host);
    }
    let output = command.output().unwrap();
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(output.status.success(), "{text}");
    assert_eq!(
        std::fs::read_to_string(home.path().join("config.toml")).unwrap(),
        config
    );
    text
}

fn assert_connect_host(text: &str, host: &str) {
    let line = text
        .lines()
        .find(|line| line.contains("Connect host:"))
        .unwrap();
    assert!(line.contains(&format!("{host}:7643")), "{line}");
}

#[test]
fn configured_host_keeps_loopback_listener() {
    let text = pair("127.0.0.1", Some("proxy.example.test"), None);
    assert_connect_host(&text, "proxy.example.test");
    assert!(
        text.lines()
            .any(|line| line.contains("Bind address:") && line.contains("127.0.0.1:7643"))
    );
}

#[test]
fn env_host_overrides_config_and_any_listener_address() {
    for bind in ["127.0.0.1", "0.0.0.0", "100.64.1.5"] {
        let text = pair(
            bind,
            Some("config.example.test"),
            Some(" env.example.test "),
        );
        assert_connect_host(&text, "env.example.test");
        assert!(
            text.lines().any(
                |line| line.contains("Bind address:") && line.contains(&format!("{bind}:7643"))
            )
        );
    }
}

#[test]
fn unset_and_blank_hosts_preserve_fallbacks() {
    assert_connect_host(&pair("127.0.0.1", None, None), "127.0.0.1");
    assert_connect_host(&pair("127.0.0.1", Some("  "), Some("  ")), "127.0.0.1");
    assert_connect_host(
        &pair("127.0.0.1", Some(" config.example.test "), Some("  ")),
        "config.example.test",
    );
}
