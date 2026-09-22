//! Black-box streamable HTTP MCP tests. The fixture uses a real loopback socket,
//! not a mocked transport, and observes the wire protocol through public APIs.

use jcode_base::mcp::{ContentBlock, McpClient, McpConfig, McpManager, McpServerConfig};
use serde_json::{Value, json};
use std::sync::{Arc, Mutex};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

#[derive(Clone, Debug)]
struct Seen {
    method: String,
    path: String,
    headers: String,
    body: Value,
}

struct Fixture {
    url: String,
    seen: Arc<Mutex<Vec<Seen>>>,
    task: tokio::task::JoinHandle<()>,
}

impl Drop for Fixture {
    fn drop(&mut self) {
        self.task.abort();
    }
}

impl Fixture {
    async fn start(sse: bool, unauthorized: bool) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}/mcp", listener.local_addr().unwrap());
        let seen = Arc::new(Mutex::new(Vec::new()));
        let records = seen.clone();
        let task = tokio::spawn(async move {
            loop {
                let Ok((stream, _)) = listener.accept().await else {
                    break;
                };
                let records = records.clone();
                tokio::spawn(async move { handle(stream, records, sse, unauthorized).await });
            }
        });
        Self { url, seen, task }
    }

    fn requests(&self) -> Vec<Seen> {
        self.seen.lock().unwrap().clone()
    }
    fn config(&self, disabled: bool) -> McpServerConfig {
        serde_json::from_value(json!({
            "type": "http", "url": self.url, "shared": false,
            "disabled": disabled, "timeout_secs": 3,
            "headers": {"Authorization": "Bearer fixture-secret", "X-Fixture": "accepted"}
        }))
        .unwrap()
    }
}

async fn handle(mut stream: TcpStream, seen: Arc<Mutex<Vec<Seen>>>, sse: bool, unauthorized: bool) {
    let mut bytes = Vec::new();
    let header_end = loop {
        let mut chunk = [0; 4096];
        let Ok(n) = stream.read(&mut chunk).await else {
            return;
        };
        if n == 0 {
            return;
        }
        bytes.extend_from_slice(&chunk[..n]);
        if let Some(pos) = bytes.windows(4).position(|x| x == b"\r\n\r\n") {
            break pos + 4;
        }
        if bytes.len() > 65536 {
            return;
        }
    };
    let header = String::from_utf8_lossy(&bytes[..header_end]).to_ascii_lowercase();
    let length = header
        .lines()
        .find_map(|line| line.strip_prefix("content-length: "))
        .and_then(|n| n.trim().parse::<usize>().ok())
        .unwrap_or(0);
    while bytes.len() - header_end < length {
        let mut chunk = [0; 4096];
        let Ok(n) = stream.read(&mut chunk).await else {
            return;
        };
        if n == 0 {
            return;
        }
        bytes.extend_from_slice(&chunk[..n]);
    }
    let body: Value =
        serde_json::from_slice(&bytes[header_end..header_end + length]).unwrap_or(Value::Null);
    let first = header.lines().next().unwrap_or("");
    let mut parts = first.split_whitespace();
    let record = Seen {
        method: parts.next().unwrap_or("").into(),
        path: parts.next().unwrap_or("").into(),
        headers: header,
        body,
    };
    let method = record.body["method"].as_str().unwrap_or("").to_owned();
    let id = record.body["id"].clone();
    seen.lock().unwrap().push(record);
    let (status, extra, payload) = if unauthorized {
        (
            "401 Unauthorized",
            "WWW-Authenticate: Bearer realm=\"fixture\"\r\n",
            String::new(),
        )
    } else if method == "notifications/initialized" {
        ("202 Accepted", "", String::new())
    } else if method == "initialize" {
        let reply = json!({"jsonrpc":"2.0","id":id,"result":{
            "protocolVersion":"2025-03-26", "capabilities":{"tools":{}},
            "serverInfo":{"name":"fixture","version":"1"}}});
        (
            "200 OK",
            "Mcp-Session-Id: fixture-session\r\n",
            reply.to_string(),
        )
    } else if method == "tools/list" {
        let reply = json!({"jsonrpc":"2.0","id":id,"result":{"tools":[{
            "name":"echo", "description":"fixture", "inputSchema":{"type":"object"}}]}});
        ("200 OK", "", encode(reply, sse))
    } else if method == "tools/call" {
        let reply = json!({"jsonrpc":"2.0","id":id,"result":{"content":[{
            "type":"text","text":record_tool_echo(&seen)}],"isError":false}});
        ("200 OK", "", encode(reply, sse))
    } else {
        ("204 No Content", "", String::new())
    };
    let content_type = if sse && (method == "tools/list" || method == "tools/call") {
        "text/event-stream"
    } else {
        "application/json"
    };
    let response = format!(
        "HTTP/1.1 {status}\r\nContent-Type: {content_type}\r\n{extra}Content-Length: {}\r\nConnection: close\r\n\r\n{payload}",
        payload.len()
    );
    let _ = stream.write_all(response.as_bytes()).await;
}

fn record_tool_echo(seen: &Arc<Mutex<Vec<Seen>>>) -> String {
    let records = seen.lock().unwrap();
    let call = records.last().unwrap();
    assert_eq!(call.body["params"]["name"], "echo");
    call.body["params"]["arguments"].to_string()
}

fn encode(reply: Value, sse: bool) -> String {
    if sse {
        format!("event: message\r\ndata: {}\r\n\r\n", reply)
    } else {
        reply.to_string()
    }
}

fn assert_wire(records: &[Seen]) {
    let methods: Vec<_> = records
        .iter()
        .filter_map(|r| r.body["method"].as_str())
        .collect();
    assert!(
        methods.starts_with(&["initialize", "notifications/initialized", "tools/list"]),
        "{methods:?}"
    );
    for record in records {
        assert_eq!(record.method, "post");
        assert_eq!(record.path, "/mcp");
        assert!(
            record
                .headers
                .contains("authorization: bearer fixture-secret")
        );
        assert!(record.headers.contains("x-fixture: accepted"));
        assert!(
            record
                .headers
                .contains("accept: application/json, text/event-stream")
        );
        if record.body["method"] != "initialize" {
            assert!(
                record.headers.contains("mcp-session-id: fixture-session"),
                "{:?}",
                record
            );
            assert!(
                record.headers.contains("mcp-protocol-version: 2025-03-26"),
                "{:?}",
                record
            );
        }
    }
}

#[tokio::test]
async fn http_client_sse_and_json_roundtrip() {
    for sse in [false, true] {
        let fixture = Fixture::start(sse, false).await;
        let mut client = tokio::time::timeout(
            std::time::Duration::from_secs(8),
            McpClient::connect("fixture".into(), &fixture.config(false)),
        )
        .await
        .unwrap()
        .unwrap();
        assert_eq!(client.tools()[0].name, "echo");
        let result = client
            .call_tool("echo", json!({"value":"roundtrip"}))
            .await
            .unwrap();
        assert!(
            matches!(&result.content[0], ContentBlock::Text { text } if text == r#"{"value":"roundtrip"}"#)
        );
        assert_wire(&fixture.requests());
        client.shutdown().await;
    }
}

#[tokio::test]
async fn manager_skips_disabled_http_but_calls_enabled_server() {
    let active = Fixture::start(true, false).await;
    let disabled = Fixture::start(false, false).await;
    let manager = McpManager::with_config(McpConfig {
        servers: [
            ("active".into(), active.config(false)),
            ("disabled".into(), disabled.config(true)),
        ]
        .into(),
    });
    let (connected, failures) = manager.connect_all().await.unwrap();
    assert_eq!(connected, 1, "{failures:?}");
    assert!(failures.is_empty(), "{failures:?}");
    let result = manager
        .call_tool("active", "echo", json!({"manager":true}))
        .await
        .unwrap();
    assert!(
        matches!(&result.content[0], ContentBlock::Text { text } if text == r#"{"manager":true}"#)
    );
    assert!(
        disabled.requests().is_empty(),
        "disabled server received network traffic"
    );
    assert_wire(&active.requests());
    manager.disconnect_all().await;
}

#[tokio::test]
async fn http_401_surfaces_auth_required_without_leaking_token() {
    let fixture = Fixture::start(false, true).await;
    let error = match tokio::time::timeout(
        std::time::Duration::from_secs(8),
        McpClient::connect("auth-fixture".into(), &fixture.config(false)),
    )
    .await
    .unwrap()
    {
        Ok(_) => panic!("401 must reject initialization"),
        Err(error) => format!("{error:#}"),
    };
    assert!(error.to_ascii_lowercase().contains("auth"), "{error}");
    assert!(
        !error.contains("fixture-secret"),
        "authentication error leaked bearer token"
    );
    assert_eq!(fixture.requests().len(), 1);
}
