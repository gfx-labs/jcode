//! Exercise the Android observer contract through the real authenticated gateway.
//! Only model inference is mocked. Owner sockets, routing, persistence and gateway
//! authentication are the production implementations.
use crate::test_support::*;
use serde_json::{Value, json};

type Socket = tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<TcpStream>>;

/// Opt-in emulator acceptance fixture. Production server and real owner transports,
/// isolated temporary JCODE_HOME, deterministic inference, loopback-only listener.
/// It never connects to the user's daemon. Read the generated code from the path
/// supplied in JCODE_MOBILE_FIXTURE_INFO. Remove that file to stop early.
#[tokio::test]
#[ignore = "long-lived, explicit emulator acceptance fixture"]
async fn mobile_observer_emulator_fixture() -> Result<()> {
    let info = std::env::var("JCODE_MOBILE_FIXTURE_INFO")
        .context("set JCODE_MOBILE_FIXTURE_INFO to a scratch JSON path")?;
    let _env = setup_test_env()?;
    let runtime = tempfile::Builder::new().prefix("mobile-ui-").tempdir()?;
    let socket_path = runtime.path().join("s.sock");
    let debug_path = runtime.path().join("d.sock");
    let port = reserve_tcp_port()?;
    let mut registry = jcode::gateway::DeviceRegistry::load();
    let code = registry.generate_pairing_code();
    registry.save()?;
    let provider = MockProvider::with_models(vec!["acceptance-fixture"]);
    for _ in 0..20 {
        provider.queue_response(vec![
            StreamEvent::TextDelta("Your message reached this isolated acceptance session through the real Jcode gateway. The terminal owner remains connected.".into()),
            StreamEvent::MessageEnd { stop_reason: Some("end_turn".into()) },
        ]);
    }
    let instance =
        server::Server::new_with_paths(Arc::new(provider), socket_path.clone(), debug_path.clone())
            .with_gateway_config(jcode::gateway::GatewayConfig {
                port,
                bind_addr: "127.0.0.1".into(),
                enabled: true,
            });
    let handle = tokio::spawn(async move { instance.run().await });
    let result = async {
        wait_for_socket(&socket_path).await?;
        wait_for_tcp_port(port).await?;
        let (mut a, sid_a) = owner(&socket_path, runtime.path()).await?;
        let (mut b, sid_b) = owner(&socket_path, runtime.path()).await?;
        let id = a
            .send_message("Verify Android gateway control without changing my terminal attachment.")
            .await?;
        collect_until_done_unix(&mut a, id).await?;
        let id = b
            .send_message("Track the second independent acceptance session.")
            .await?;
        collect_until_done_unix(&mut b, id).await?;
        std::fs::write(
            &info,
            serde_json::to_vec_pretty(&json!({
                "port": port, "code": code, "sessions": [sid_a, sid_b],
                "host": format!("10.0.2.2:{port}"), "fixture": true
            }))?,
        )?;
        let until = Instant::now() + Duration::from_secs(900);
        while Instant::now() < until && std::path::Path::new(&info).exists() {
            // Drain actual owner events, proving the phone does not need takeover.
            tokio::select! {
                _ = a.read_event() => {},
                _ = b.read_event() => {},
                _ = tokio::time::sleep(Duration::from_secs(1)) => {},
            }
        }
        Ok::<_, anyhow::Error>(())
    }
    .await;
    abort_server_and_cleanup(&handle, &socket_path, &debug_path);
    result
}

async fn connect_observer(port: u16, token: &str) -> Result<Socket> {
    let mut request = format!("ws://127.0.0.1:{port}/ws").into_client_request()?;
    request
        .headers_mut()
        .insert("Authorization", format!("Bearer {token}").parse()?);
    Ok(connect_async(request).await?.0)
}

async fn exchange(port: u16, token: &str, request: Value, response_type: &str) -> Result<Value> {
    let mut socket = connect_observer(port, token).await?;
    let id = request["id"].clone();
    socket.send(WsMessage::Text(request.to_string())).await?;
    timeout(Duration::from_secs(15), async {
        while let Some(frame) = socket.next().await {
            match frame? {
                WsMessage::Text(text) => {
                    for line in text.lines().filter(|line| !line.trim().is_empty()) {
                        let event: Value = serde_json::from_str(line)?;
                        if event["id"] == id {
                            if event["type"] == response_type {
                                return Ok(event);
                            }
                            if event["type"] == "error" {
                                anyhow::bail!("observer request failed: {event}");
                            }
                        }
                    }
                }
                WsMessage::Ping(payload) => socket.send(WsMessage::Pong(payload)).await?,
                WsMessage::Close(_) => anyhow::bail!("observer disconnected"),
                _ => {}
            }
        }
        anyhow::bail!("observer stream ended")
    })
    .await?
}

async fn owner(path: &std::path::Path, cwd: &std::path::Path) -> Result<(server::Client, String)> {
    let mut client = server::Client::connect_with_path(path.to_owned()).await?;
    let id = client
        .subscribe_with_info(Some(cwd.display().to_string()), None, None, false, false)
        .await?;
    collect_until_done_unix(&mut client, id).await?;
    match client.get_history_event().await? {
        ServerEvent::History { session_id, .. } => Ok((client, session_id)),
        other => anyhow::bail!("unexpected owner history: {other:?}"),
    }
}

#[tokio::test]
async fn mobile_observer_global_discovery_delivery_and_owner_isolation() -> Result<()> {
    let _env = setup_test_env()?;
    let runtime = tempfile::Builder::new().prefix("mobile-e2e-").tempdir()?;
    let socket_path = runtime.path().join("s.sock");
    let debug_path = runtime.path().join("d.sock");
    let cwd_a = runtime.path().join("project-a");
    let cwd_b = runtime.path().join("project-b");
    std::fs::create_dir_all(&cwd_a)?;
    std::fs::create_dir_all(&cwd_b)?;
    let port = reserve_tcp_port()?;
    let mut registry = jcode::gateway::DeviceRegistry::load();
    let token = registry.pair_device("android-e2e".into(), "Android acceptance".into(), None);
    registry.save()?;
    let provider = MockProvider::new();
    for _ in 0..4 {
        provider.queue_response(vec![
            StreamEvent::TextDelta("Mobile delivery reached its original owner.".into()),
            StreamEvent::MessageEnd {
                stop_reason: Some("end_turn".into()),
            },
        ]);
    }
    let instance =
        server::Server::new_with_paths(Arc::new(provider), socket_path.clone(), debug_path.clone())
            .with_gateway_config(jcode::gateway::GatewayConfig {
                port,
                bind_addr: "127.0.0.1".into(),
                enabled: true,
            });
    let handle = tokio::spawn(async move { instance.run().await });
    let result = async {
        wait_for_socket(&socket_path).await?;
        wait_for_tcp_port(port).await?;
        assert!(connect_observer(port, &"0".repeat(64)).await.is_err(), "unknown token must not observe sessions");
        let empty = exchange(port, &token, json!({"type":"list_sessions","id":1}), "sessions_list").await?;
        assert_eq!(empty["sessions"].as_array().map(Vec::len), Some(0), "observation must not create a session");
        let (mut owner_a, sid_a) = owner(&socket_path, &cwd_a).await?;
        let (mut owner_b, sid_b) = owner(&socket_path, &cwd_b).await?;
        let list = exchange(port, &token, json!({"type":"list_sessions","id":2}), "sessions_list").await?;
        let sessions = list["sessions"].as_array().context("sessions array")?;
        assert_eq!(sessions.len(), 2, "global list must include both independent non-git owners only");
        for sid in [&sid_a, &sid_b] {
            assert!(sessions.iter().any(|s| s["session_id"] == *sid), "missing owner {sid}: {list}");
        }
        for (id, sid) in [(3, &sid_a), (4, &sid_b)] {
            let delivery = exchange(port, &token, json!({"type":"mobile_message","id":id,"session_id":sid,"content":"Mobile acceptance message"}), "mobile_delivery").await?;
            assert_eq!(delivery["status"], "accepted", "{delivery}");
            assert_eq!(delivery["session_id"], *sid);
        }
        let rejected = exchange(port, &token, json!({"type":"mobile_message","id":5,"session_id":"nonexistent-mobile-session","content":"Never create this target"}), "mobile_delivery").await?;
        assert_eq!(rejected["status"], "rejected");
        let blank = exchange(port, &token, json!({"type":"mobile_message","id":6,"session_id":sid_a,"content":"   "}), "mobile_delivery").await?;
        assert_eq!(blank["status"], "rejected");

        // Both original owner transports still function after observing and sending.
        // Poll persisted history because delivery acceptance precedes a model turn.
        for (client, sid) in [(&mut owner_a, &sid_a), (&mut owner_b, &sid_b)] {
            timeout(Duration::from_secs(15), async {
                loop {
                    let event = client.get_history_event().await?;
                    if let ServerEvent::History { session_id, messages, .. } = event {
                        assert_eq!(&session_id, sid, "mobile must not steal or replace the owner");
                        let text = serde_json::to_string(&messages)?;
                        if text.contains("Mobile acceptance message") { break; }
                    }
                    tokio::time::sleep(Duration::from_millis(100)).await;
                }
                Ok::<_, anyhow::Error>(())
            }).await??;
        }
        let context = exchange(port, &token, json!({"type":"comm_read_context","id":7,"session_id":sid_a,"target_session":sid_a}), "comm_context_history").await?;
        assert_eq!(context["session_id"], sid_a);
        assert!(context["messages"].to_string().contains("Mobile acceptance message"));
        let list = exchange(port, &token, json!({"type":"list_sessions","id":8}), "sessions_list").await?;
        assert_eq!(list["sessions"].as_array().map(Vec::len), Some(2), "observer reconnect must not leave phantom sessions");
        // Explicit disconnect is not a stop/cancel signal for an owner.
        assert!(matches!(owner_a.get_history_event().await?, ServerEvent::History { session_id, .. } if session_id == sid_a));
        Ok::<_, anyhow::Error>(())
    }.await;
    abort_server_and_cleanup(&handle, &socket_path, &debug_path);
    result
}
