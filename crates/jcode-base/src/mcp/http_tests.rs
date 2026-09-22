use super::*;
use crate::mcp::test_http_mock::{self, Reply};
use std::sync::Arc;

fn cfg(url: &str) -> McpServerConfig {
    serde_json::from_value(serde_json::json!({ "type": "http", "url": url })).unwrap()
}

#[test]
fn sse_decoder_handles_split_chunks_and_multiline() {
    let mut d = SseDecoder::default();
    assert!(d.feed(b"event: message\r\ndata: {\"jsonrpc\":\"2.0\",").unwrap().is_empty());
    let ev = d.feed(b"\r\ndata: \"id\":1,\"result\":{}}\r\n\r\n").unwrap();
    assert_eq!(ev.len(), 1);
    let parsed = parse_json_messages(&ev[0]);
    assert_eq!(parsed[0].id, Some(1));
}

#[test]
fn parse_sse_skips_notifications_and_batches() {
    let body = "data: {\"jsonrpc\":\"2.0\",\"method\":\"notifications/progress\"}\n\n\
                data: [{\"jsonrpc\":\"2.0\",\"id\":2,\"result\":{}}]\n\n";
    let msgs = parse_sse_messages(body);
    assert!(msgs.iter().any(|m| m.id == Some(2)));
}

#[test]
fn reserved_headers_are_dropped() {
    let mut h = HashMap::new();
    h.insert("Mcp-Session-Id".to_string(), "evil".to_string());
    h.insert("Accept".to_string(), "x".to_string());
    h.insert("X-Api-Key".to_string(), "k".to_string());
    let out = sanitize_headers(&h);
    assert_eq!(out, vec![("X-Api-Key".to_string(), "k".to_string())]);
}

#[tokio::test]
async fn sse_request_returns_without_waiting_for_stream_close() {
    let server = test_http_mock::start(Arc::new(|_| Reply {
        status: 200,
        headers: vec![("content-type".into(), "text/event-stream".into())],
        body: "data: {\"jsonrpc\":\"2.0\",\"id\":7,\"result\":{\"ok\":true}}\n\n".into(),
        hold_open: true,
    }))
    .await;
    let t = HttpTransport::new("t".into(), &cfg(&format!("{}/mcp", server.base))).unwrap();
    let r = tokio::time::timeout(std::time::Duration::from_secs(5), t.request(7, "{}".into()))
        .await
        .expect("must not hang on open SSE stream")
        .unwrap();
    assert_eq!(r.result.unwrap()["ok"], true);
}

#[tokio::test]
async fn initialize_session_and_protocol_headers_roundtrip() {
    let server = test_http_mock::start(Arc::new(|req| {
        let v: Value = serde_json::from_str(&req.body).unwrap_or_default();
        let method = v["method"].as_str().unwrap_or_default().to_string();
        let id = v["id"].clone();
        match method.as_str() {
            "initialize" => Reply::json(
                200,
                serde_json::json!({"jsonrpc":"2.0","id":id,"result":{
                    "protocolVersion":"2025-06-18","capabilities":{},
                    "serverInfo":{"name":"mock","version":"1"}}})
                .to_string(),
            )
            .header("Mcp-Session-Id", "sess-1"),
            "notifications/initialized" => Reply::json(202, ""),
            "tools/list" => Reply::json(
                200,
                serde_json::json!({"jsonrpc":"2.0","id":id,"result":{"tools":[
                    {"name":"echo","inputSchema":{"type":"object"}}]}})
                .to_string(),
            ),
            _ => Reply::json(400, "{}"),
        }
    }))
    .await;
    let client = crate::mcp::McpClient::connect("mock".into(), &cfg(&format!("{}/mcp", server.base)))
        .await
        .unwrap();
    assert_eq!(client.tools().len(), 1);
    let reqs = server.requests.lock().unwrap().clone();
    assert_eq!(reqs.len(), 3);
    assert!(reqs[0].headers.get("mcp-session-id").is_none());
    assert!(reqs[0].headers["accept"].contains("text/event-stream"));
    // Ordered: initialized notification completes before tools/list.
    assert!(reqs[1].body.contains("notifications/initialized"));
    for r in &reqs[1..] {
        assert_eq!(r.headers["mcp-session-id"], "sess-1");
        assert_eq!(r.headers["mcp-protocol-version"], "2025-06-18");
    }
}

#[tokio::test]
async fn unauthorized_maps_to_auth_required_error() {
    let server = test_http_mock::start(Arc::new(|_| {
        Reply::json(401, "{}").header(
            "WWW-Authenticate",
            "Bearer resource_metadata=\"http://127.0.0.1:1/.well-known/oauth-protected-resource\"",
        )
    }))
    .await;
    let err = crate::mcp::McpClient::connect("fig".into(), &cfg(&format!("{}/mcp", server.base)))
        .await
        .err()
        .unwrap();
    assert!(oauth::is_auth_required_error(&err), "{err:#}");
}

#[test]
fn insecure_remote_url_rejected() {
    assert!(HttpTransport::new("x".into(), &cfg("http://example.com/mcp")).is_err());
    assert!(HttpTransport::new("x".into(), &cfg("https://u:p@example.com/mcp")).is_err());
    assert!(HttpTransport::new("x".into(), &cfg("http://127.0.0.1:9/mcp")).is_ok());
}
