#![allow(clippy::collapsible_match)]

use super::*;
use anyhow::Result;
use futures::{SinkExt, StreamExt};
use jcode_base::auth::codex::CodexCredentials;
use jcode_message_types::{ContentBlock, Role};
use std::collections::{HashMap, HashSet};
use std::ffi::OsString;
use std::path::PathBuf;
use std::sync::MutexGuard;
use std::time::{Duration, Instant};
const BRIGHT_PEARL_WRAPPED_TOOL_CALL_FIXTURE: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../tests/fixtures/openai/bright_pearl_wrapped_tool_call.txt"
));

struct EnvVarGuard {
    key: &'static str,
    previous: Option<OsString>,
}

impl EnvVarGuard {
    fn set(key: &'static str, value: &str) -> Self {
        let previous = std::env::var_os(key);
        jcode_base::env::set_var(key, value);
        Self { key, previous }
    }

    fn set_path(key: &'static str, value: &std::path::Path) -> Self {
        let previous = std::env::var_os(key);
        jcode_base::env::set_var(key, value);
        Self { key, previous }
    }

    fn remove(key: &'static str) -> Self {
        let previous = std::env::var_os(key);
        jcode_base::env::remove_var(key);
        Self { key, previous }
    }
}

impl Drop for EnvVarGuard {
    fn drop(&mut self) {
        if let Some(previous) = &self.previous {
            jcode_base::env::set_var(self.key, previous);
        } else {
            jcode_base::env::remove_var(self.key);
        }
    }
}

pub(super) async fn test_persistent_ws_state() -> (PersistentWsState, tokio::task::JoinHandle<()>) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind test websocket listener");
    let addr = listener.local_addr().expect("listener local addr");
    let server = tokio::spawn(async move {
        let (stream, _) = listener.accept().await.expect("accept websocket client");
        let mut ws = tokio_tungstenite::accept_async(stream)
            .await
            .expect("accept websocket handshake");
        while let Some(message) = ws.next().await {
            match message {
                Ok(WsMessage::Ping(payload)) => {
                    let _ = ws.send(WsMessage::Pong(payload)).await;
                }
                Ok(WsMessage::Close(_)) | Err(_) => break,
                _ => {}
            }
        }
    });

    let (client_ws, _) = connect_async(format!("ws://{}", addr))
        .await
        .expect("connect websocket client");
    (
        PersistentWsState {
            ws_stream: client_ws,
            identity: openai_websocket_prewarm::prewarm_identity(&prewarm_test_credentials()),
            last_response_id: "resp_test".to_string(),
            connected_at: Instant::now(),
            last_activity_at: Instant::now(),
            last_response_completed_at: Instant::now(),
            message_count: 1,
            last_input_item_count: 1,
        },
        server,
    )
}

async fn test_persistent_ws_state_with_ping_notify() -> (
    PersistentWsState,
    tokio::task::JoinHandle<()>,
    Arc<tokio::sync::Notify>,
    Arc<tokio::sync::Notify>,
) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind test websocket listener");
    let addr = listener.local_addr().expect("listener local addr");
    let ping_notify = Arc::new(tokio::sync::Notify::new());
    let server_ping_notify = Arc::clone(&ping_notify);
    let pong_notify = Arc::new(tokio::sync::Notify::new());
    let server_pong_notify = Arc::clone(&pong_notify);
    let server = tokio::spawn(async move {
        let (stream, _) = listener.accept().await.expect("accept websocket client");
        let mut ws = tokio_tungstenite::accept_async(stream)
            .await
            .expect("accept websocket handshake");
        while let Some(message) = ws.next().await {
            match message {
                Ok(WsMessage::Ping(payload)) => {
                    server_ping_notify.notify_one();
                    let _ = ws.send(WsMessage::Pong(b"stale-pong".to_vec())).await;
                    let _ = ws.send(WsMessage::Ping(b"server-keepalive".to_vec())).await;
                    let _ = ws.send(WsMessage::Pong(payload)).await;
                }
                Ok(WsMessage::Pong(payload)) if payload.as_slice() == b"server-keepalive" => {
                    server_pong_notify.notify_one();
                }
                Ok(WsMessage::Close(_)) | Err(_) => break,
                _ => {}
            }
        }
    });

    let (client_ws, _) = connect_async(format!("ws://{}", addr))
        .await
        .expect("connect websocket client");
    (
        PersistentWsState {
            ws_stream: client_ws,
            identity: openai_websocket_prewarm::prewarm_identity(&prewarm_test_credentials()),
            last_response_id: "resp_test".to_string(),
            connected_at: Instant::now(),
            last_activity_at: Instant::now(),
            last_response_completed_at: Instant::now(),
            message_count: 1,
            last_input_item_count: 1,
        },
        server,
        ping_notify,
        pong_notify,
    )
}

struct LiveOpenAITestEnv {
    _lock: MutexGuard<'static, ()>,
    _jcode_home: EnvVarGuard,
    _transport: EnvVarGuard,
    _temp: tempfile::TempDir,
}

impl LiveOpenAITestEnv {
    fn new() -> Result<Option<Self>> {
        let lock = jcode_base::storage::lock_test_env();
        let Some(source_auth) = real_codex_auth_path() else {
            return Ok(None);
        };

        let temp = tempfile::Builder::new()
            .prefix("jcode-openai-live-")
            .tempdir()?;
        let target_auth = temp
            .path()
            .join("external")
            .join(".codex")
            .join("auth.json");
        std::fs::create_dir_all(
            target_auth
                .parent()
                .expect("temp auth target should have a parent"),
        )?;
        std::fs::copy(source_auth, &target_auth)?;

        let jcode_home = EnvVarGuard::set_path("JCODE_HOME", temp.path());
        let transport = EnvVarGuard::set("JCODE_OPENAI_TRANSPORT", "https");

        Ok(Some(Self {
            _lock: lock,
            _jcode_home: jcode_home,
            _transport: transport,
            _temp: temp,
        }))
    }
}

fn real_codex_auth_path() -> Option<PathBuf> {
    let home = dirs::home_dir()?;
    let path = home.join(".codex").join("auth.json");
    path.exists().then_some(path)
}

async fn live_openai_catalog() -> Result<Option<jcode_base::provider::OpenAIModelCatalog>> {
    let Some(_env) = LiveOpenAITestEnv::new()? else {
        return Ok(None);
    };
    let creds = jcode_base::auth::codex::load_credentials()?;
    if !OpenAIProvider::is_chatgpt_mode(&creds) {
        return Ok(None);
    }

    let token = openai_access_token(&Arc::new(RwLock::new(creds))).await?;
    Ok(Some(
        jcode_base::provider::fetch_openai_model_catalog(&token).await?,
    ))
}

async fn live_openai_smoke(model: &str, sentinel: &str) -> Result<Option<String>> {
    let Some(_env) = LiveOpenAITestEnv::new()? else {
        return Ok(None);
    };
    let creds = jcode_base::auth::codex::load_credentials()?;
    if !OpenAIProvider::is_chatgpt_mode(&creds) {
        return Ok(None);
    }

    let provider = OpenAIProvider::new(creds);
    provider.set_model(model)?;
    let response = provider
        .complete_simple(&format!("Reply with exactly {}.", sentinel), "")
        .await?;
    Ok(Some(response))
}

include!("openai_tests/models_state.rs");
include!("openai_tests/responses_input.rs");
include!("openai_tests/transport_runtime.rs");
include!("openai_tests/websocket_prewarm.rs");
include!("openai_tests/payloads.rs");
include!("openai_tests/parsing_tools.rs");

/// Mirror of the Anthropic round-trip guard: the runtime-provider identity that
/// `set_credential_mode` writes for OpenAI must decode back to the same mode so
/// the model picker / header widget report the auth method that requests will
/// actually use.
#[test]
fn openai_credential_mode_runtime_provider_identity_round_trips() {
    let _guard = jcode_base::storage::lock_test_env();
    let previous = std::env::var_os("JCODE_RUNTIME_PROVIDER");

    jcode_base::env::set_var("JCODE_RUNTIME_PROVIDER", "openai");
    assert_eq!(
        OpenAICredentialMode::from_runtime_env(jcode_provider_core::DualAuthProvider::OpenAI),
        OpenAICredentialMode::OAuth,
        "OAuth selection must surface as the OAuth runtime identity"
    );

    jcode_base::env::set_var("JCODE_RUNTIME_PROVIDER", "openai-api");
    assert_eq!(
        OpenAICredentialMode::from_runtime_env(jcode_provider_core::DualAuthProvider::OpenAI),
        OpenAICredentialMode::ApiKey,
        "API-key selection must surface as the API-key runtime identity"
    );

    match previous {
        Some(value) => jcode_base::env::set_var("JCODE_RUNTIME_PROVIDER", value),
        None => jcode_base::env::remove_var("JCODE_RUNTIME_PROVIDER"),
    }
}

#[tokio::test]
async fn openai_available_efforts_follow_active_model_catalog_metadata() {
    let provider = OpenAIProvider::new_browser_only();
    *provider.model.write().await = "gpt-5.6".to_string();
    provider
        .model_reasoning_efforts
        .write()
        .expect("reasoning effort catalog lock")
        .insert(
            "gpt-5.6".to_string(),
            vec![
                "minimal".to_string(),
                "medium".to_string(),
                "max".to_string(),
            ],
        );

    assert_eq!(
        provider.available_efforts(),
        vec!["minimal", "medium", "max", "swarm", "swarm-deep"]
    );
    *provider.model.write().await = "gpt-5.6[1m]".to_string();
    assert_eq!(
        provider.available_efforts(),
        vec!["minimal", "medium", "max", "swarm", "swarm-deep"],
        "long-context aliases must use the canonical model's catalog metadata"
    );
    *provider.model.write().await = "gpt-5.6".to_string();
    assert_eq!(
        provider.api_reasoning_effort(Some("swarm")).as_deref(),
        Some("max")
    );

    provider
        .model_reasoning_efforts
        .write()
        .expect("reasoning effort catalog lock")
        .insert(
            "gpt-5.6".to_string(),
            vec!["low".to_string(), "high".to_string(), "xhigh".to_string()],
        );
    assert_eq!(
        provider.api_reasoning_effort(Some("swarm")).as_deref(),
        Some("xhigh"),
        "swarm must clamp to the active model's strongest advertised effort"
    );
    assert!(
        provider.set_reasoning_effort("max").is_err(),
        "explicit effort choices must respect active-model catalog capabilities"
    );
    provider
        .set_reasoning_effort("xhigh")
        .expect("advertised effort should be accepted");
    provider
        .model_reasoning_efforts
        .write()
        .expect("reasoning effort catalog lock")
        .insert(
            "gpt-5.5".to_string(),
            vec!["low".to_string(), "high".to_string()],
        );
    *provider.model.write().await = "gpt-5.5".to_string();
    provider.revalidate_reasoning_effort();
    assert_eq!(
        provider.reasoning_effort(),
        None,
        "an effort unsupported by the newly selected model must not remain active"
    );
    assert!(provider.set_reasoning_effort("typo").is_err());
}

#[test]
fn catalog_credential_identity_survives_token_refresh_but_changes_accounts() {
    let credentials = |access: &str, refresh: &str, account: Option<&str>| CodexCredentials {
        access_token: access.to_string(),
        refresh_token: refresh.to_string(),
        id_token: None,
        account_id: account.map(str::to_string),
        expires_at: None,
    };
    assert_eq!(
        OpenAIProvider::catalog_credential_identity(&credentials("old", "refresh", Some("acct"))),
        OpenAIProvider::catalog_credential_identity(&credentials("new", "refresh", Some("acct")))
    );
    assert_ne!(
        OpenAIProvider::catalog_credential_identity(&credentials("old", "refresh-a", None)),
        OpenAIProvider::catalog_credential_identity(&credentials("new", "refresh-b", None))
    );
}

#[tokio::test]
async fn agent_instructions_openai_snapshot_is_available_and_independent() {
    let provider = OpenAIProvider::new_inner(prewarm_test_credentials(), false);
    *provider.model.write().await = "gpt-5.4".into();
    let snapshot = provider.fork_for_instruction_generation();
    assert!(
        snapshot.is_ok(),
        "direct Responses route needs an isolated snapshot"
    );
    let snapshot = snapshot.unwrap();
    assert_eq!(snapshot.model(), provider.model());
    *provider.model.write().await = "different-model".into();
    assert_eq!(snapshot.model(), "gpt-5.4");
}

#[tokio::test]
async fn agent_instructions_openai_omits_hosted_tools_and_preserves_exact_model() {
    let provider = OpenAIProvider::new_inner(prewarm_test_credentials(), false);
    *provider.model.write().await = "wizard-unknown-model".into();
    let isolated = provider.instruction_generation_snapshot();
    assert!(
        isolated.is_ok(),
        "isolated request construction must be available"
    );
    let isolated = isolated.unwrap();
    let input = build_responses_input(&[ChatMessage::user("Draft instructions")]);
    let request =
        isolated.response_request_for_model("gpt-5.4", &input, &[], "Only instructions", true);
    assert_eq!(request["tools"], serde_json::json!([]));
    assert!(request.get("previous_response_id").is_none());
    assert_eq!(request["instructions"], "Only instructions");
    assert_eq!(request["input"], serde_json::json!(input));
    assert_eq!(isolated.model_id().await, "wizard-unknown-model");
    assert!(isolated.persistent_ws.lock().await.is_none());
}

#[tokio::test]
async fn agent_instructions_openai_cancel_closes_owned_transport() {
    let _lock = jcode_base::storage::lock_test_env();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let _base = EnvVarGuard::set("JCODE_OPENAI_API_BASE", &format!("http://{addr}/v1"));
    let _transport = EnvVarGuard::set("JCODE_OPENAI_TRANSPORT", "websocket");
    let (seen_tx, seen_rx) = tokio::sync::oneshot::channel();
    let (closed_tx, closed_rx) = tokio::sync::oneshot::channel();
    let server = tokio::spawn(async move {
        let (stream, _) = listener.accept().await.unwrap();
        let mut socket = tokio_tungstenite::accept_async(stream).await.unwrap();
        let request: Value = serde_json::from_str(
            socket
                .next()
                .await
                .unwrap()
                .unwrap()
                .into_text()
                .unwrap()
                .as_ref(),
        )
        .unwrap();
        assert_eq!(request["tools"], serde_json::json!([]));
        assert_eq!(request["model"], "gpt-5.4");
        assert!(request.get("previous_response_id").is_none());
        assert_eq!(request["input"].as_array().unwrap().len(), 1);
        seen_tx.send(()).unwrap();
        let closed = matches!(
            socket.next().await,
            None | Some(Err(_)) | Some(Ok(WsMessage::Close(_)))
        );
        let _ = closed_tx.send(closed);
    });
    let provider = OpenAIProvider::new_inner(prewarm_test_credentials(), false);
    *provider.model.write().await = "gpt-5.4".into();
    let generation = tokio::spawn(jcode_base::agent_instructions::generate_agent_instructions(
        Arc::new(provider),
        "Reviewer".into(),
        "Review code".into(),
        "all".into(),
    ));
    tokio::time::timeout(Duration::from_secs(2), seen_rx)
        .await
        .unwrap()
        .unwrap();
    generation.abort();
    let _ = generation.await;
    let closed = tokio::time::timeout(Duration::from_millis(300), closed_rx).await;
    server.abort();
    assert!(
        matches!(closed, Ok(Ok(true))),
        "cancelling generation must abort its transport task"
    );
}

#[tokio::test]
async fn agent_instructions_openai_snapshot_does_not_share_reasoning_catalog() {
    let provider = OpenAIProvider::new_inner(prewarm_test_credentials(), false);
    let snapshot = provider.instruction_generation_snapshot().unwrap();
    assert!(!Arc::ptr_eq(
        &provider.model_reasoning_efforts,
        &snapshot.model_reasoning_efforts
    ));
}

#[tokio::test]
async fn agent_instructions_openai_error_logs_never_echo_body() {
    let _lock = jcode_base::storage::lock_test_env();
    jcode_base::logging::init();
    let path = jcode_base::logging::log_path().expect("test log path");
    let sentinel = format!(
        "INSTRUCTION_PRIVATE_ECHO_{}",
        jcode_base::id::new_id("privacy")
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let _base = EnvVarGuard::set("JCODE_OPENAI_API_BASE", &format!("http://{addr}/v1"));
    let _transport = EnvVarGuard::set("JCODE_OPENAI_TRANSPORT", "websocket");
    let echo = sentinel.clone();
    let server = tokio::spawn(async move {
        let (stream, _) = listener.accept().await.unwrap();
        let mut socket = tokio_tungstenite::accept_async(stream).await.unwrap();
        socket.next().await.unwrap().unwrap();
        socket.send(WsMessage::Text(serde_json::json!({"type":"error","error":{"code":"invalid_request_error","message":echo}}).to_string().into())).await.unwrap();
        let _ = socket.close(None).await;
    });
    let provider = Arc::new(OpenAIProvider::new_inner(prewarm_test_credentials(), false));
    *provider.model.write().await = "gpt-5.4".into();
    let result = tokio::time::timeout(
        std::time::Duration::from_secs(3),
        jcode_base::agent_instructions::generate_agent_instructions(
            provider,
            "Reviewer".into(),
            sentinel.clone(),
            "all".into(),
        ),
    )
    .await
    .unwrap();
    server.await.unwrap();
    assert!(result.is_err());
    assert!(!result.unwrap_err().to_string().contains(&sentinel));
    let log = std::fs::read_to_string(path).unwrap();
    assert!(
        !log.contains(&sentinel),
        "provider diagnostic logs must not contain echoed purpose text"
    );
}
