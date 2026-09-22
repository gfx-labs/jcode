//! Native MCP "streamable HTTP" transport (MCP spec 2025-03-26 / 2025-06-18).
//!
//! Every JSON-RPC message is POSTed to the server URL. The server replies with
//! either `application/json` (a single message or batch) or
//! `text/event-stream`. SSE bodies are parsed incrementally and the request
//! completes as soon as the matching response id arrives, so servers that keep
//! the stream open do not hang the client. The `Mcp-Session-Id` header returned
//! by `initialize` is echoed on every later request, and the negotiated
//! protocol version is sent as `MCP-Protocol-Version`. Bearer tokens come from
//! [`super::oauth`] and are refreshed transparently.
//!
//! Requests are driven inline by the caller (no background dispatcher), so the
//! caller's timeout cancels the HTTP exchange and message ordering (initialize,
//! then `notifications/initialized`, then later requests) is preserved.

use super::oauth;
use super::protocol::{JsonRpcResponse, McpServerConfig};
use anyhow::{Context, Result, bail};
use futures::StreamExt;
use serde_json::Value;
use std::collections::HashMap;

pub(crate) const SESSION_HEADER: &str = "mcp-session-id";
pub(crate) const PROTOCOL_HEADER: &str = "mcp-protocol-version";
/// Upper bound on a JSON body or a single SSE event.
pub(crate) const MAX_MESSAGE_BYTES: usize = 16 * 1024 * 1024;

/// Headers owned by the transport. Config entries with these names are ignored.
const RESERVED_HEADERS: &[&str] = &[
    SESSION_HEADER,
    PROTOCOL_HEADER,
    "content-type",
    "accept",
    "content-length",
    "host",
];

/// Whether a server config should use the HTTP transport.
pub fn is_http_config(config: &McpServerConfig) -> bool {
    config.is_http()
}

pub(crate) struct HttpTransport {
    pub(crate) name: String,
    url: String,
    client: reqwest::Client,
    headers: Vec<(String, String)>,
    has_explicit_auth: bool,
    config: McpServerConfig,
    session_id: std::sync::RwLock<Option<String>>,
    protocol_version: std::sync::RwLock<Option<String>>,
}

/// Filter configured headers: drop transport-owned names.
pub(crate) fn sanitize_headers(headers: &HashMap<String, String>) -> Vec<(String, String)> {
    let mut out: Vec<(String, String)> = headers
        .iter()
        .filter(|(k, _)| !RESERVED_HEADERS.iter().any(|r| k.eq_ignore_ascii_case(r)))
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect();
    out.sort();
    out
}

impl HttpTransport {
    pub(crate) fn new(name: String, config: &McpServerConfig) -> Result<Self> {
        let url = config
            .url
            .clone()
            .filter(|u| !u.trim().is_empty())
            .context("HTTP MCP server has no url")?;
        oauth::validate_endpoint(&url).context("Invalid MCP server url")?;
        let client = reqwest::Client::builder()
            // Never follow redirects: they could forward the bearer token to
            // another origin.
            .redirect(reqwest::redirect::Policy::none())
            .connect_timeout(std::time::Duration::from_secs(15))
            .build()?;
        let headers = sanitize_headers(&config.headers);
        let has_explicit_auth = headers
            .iter()
            .any(|(k, _)| k.eq_ignore_ascii_case("authorization"));
        Ok(Self {
            name,
            url,
            client,
            headers,
            has_explicit_auth,
            config: config.clone(),
            session_id: std::sync::RwLock::new(None),
            protocol_version: std::sync::RwLock::new(None),
        })
    }

    pub(crate) fn session_id(&self) -> Option<String> {
        self.session_id
            .read()
            .unwrap_or_else(|p| p.into_inner())
            .clone()
    }

    pub(crate) fn set_protocol_version(&self, version: &str) {
        *self
            .protocol_version
            .write()
            .unwrap_or_else(|p| p.into_inner()) = Some(version.to_string());
    }

    async fn authorize(&self, mut req: reqwest::RequestBuilder) -> Result<reqwest::RequestBuilder> {
        for (k, v) in &self.headers {
            req = req.header(k, v);
        }
        if !self.has_explicit_auth
            && let Some(token) = oauth::access_token(&self.config).await?
        {
            req = req.bearer_auth(token);
        }
        if let Some(sid) = self.session_id() {
            req = req.header(SESSION_HEADER, sid);
        }
        if let Some(v) = self
            .protocol_version
            .read()
            .unwrap_or_else(|p| p.into_inner())
            .clone()
        {
            req = req.header(PROTOCOL_HEADER, v);
        }
        Ok(req)
    }

    async fn send(&self, body: String) -> Result<reqwest::Response> {
        let req = self
            .client
            .post(&self.url)
            .header("content-type", "application/json")
            .header("accept", "application/json, text/event-stream")
            .body(body);
        let resp = self
            .authorize(req)
            .await?
            .send()
            .await
            .with_context(|| format!("HTTP request to MCP server '{}' failed", self.name))?;
        let status = resp.status();
        if let Some(sid) = resp
            .headers()
            .get(SESSION_HEADER)
            .and_then(|v| v.to_str().ok())
        {
            *self.session_id.write().unwrap_or_else(|p| p.into_inner()) = Some(sid.to_string());
        }
        if status == reqwest::StatusCode::UNAUTHORIZED {
            if let Some(c) = resp
                .headers()
                .get("www-authenticate")
                .and_then(|v| v.to_str().ok())
            {
                oauth::remember_challenge(&self.config, c);
            }
            bail!(oauth::auth_required_message(&self.name));
        }
        if status == reqwest::StatusCode::NOT_FOUND && self.session_id().is_some() {
            *self.session_id.write().unwrap_or_else(|p| p.into_inner()) = None;
            bail!("MCP server '{}' session expired (404)", self.name);
        }
        if status.is_redirection() {
            bail!(
                "MCP server '{}' answered with redirect HTTP {status}; update the configured url",
                self.name
            );
        }
        if !status.is_success() {
            let text = read_limited(resp, 4096).await.unwrap_or_default();
            bail!(
                "MCP server '{}' returned HTTP {}: {}",
                self.name,
                status,
                text
            );
        }
        Ok(resp)
    }

    /// POST a request and wait for the response with `id`.
    pub(crate) async fn request(&self, id: u64, body: String) -> Result<JsonRpcResponse> {
        let resp = self.send(body).await?;
        let is_sse = content_type(&resp).starts_with("text/event-stream");
        if is_sse {
            let stream = resp.bytes_stream().map(|r| r.map_err(anyhow::Error::from));
            read_sse_until(stream, id).await
        } else {
            let text = read_limited(resp, MAX_MESSAGE_BYTES).await?;
            parse_json_messages(&text)
                .into_iter()
                .find(|r| r.id == Some(id))
                .with_context(|| {
                    format!(
                        "MCP server '{}' returned no response for id {id}",
                        self.name
                    )
                })
        }
    }

    /// POST a notification. Completes once the server accepted it.
    pub(crate) async fn notify(&self, body: String) -> Result<()> {
        let _ = self.send(body).await?;
        Ok(())
    }

    /// Best-effort session termination (DELETE with the session header).
    pub(crate) async fn terminate(&self) {
        if self.session_id().is_none() {
            return;
        }
        let Ok(req) = self.authorize(self.client.delete(&self.url)).await else {
            return;
        };
        let _ = tokio::time::timeout(std::time::Duration::from_secs(3), req.send()).await;
    }
}

fn content_type(resp: &reqwest::Response) -> String {
    resp.headers()
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_ascii_lowercase()
}

async fn read_limited(resp: reqwest::Response, limit: usize) -> Result<String> {
    let mut stream = resp.bytes_stream();
    let mut buf = Vec::new();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk?;
        if buf.len() + chunk.len() > limit {
            if limit < MAX_MESSAGE_BYTES {
                buf.extend_from_slice(&chunk[..limit - buf.len()]);
                break;
            }
            bail!("MCP response exceeded {limit} bytes");
        }
        buf.extend_from_slice(&chunk);
    }
    Ok(String::from_utf8_lossy(&buf).into_owned())
}

/// Parse a JSON body holding a single message or a batch.
pub(crate) fn parse_json_messages(text: &str) -> Vec<JsonRpcResponse> {
    match serde_json::from_str::<Value>(text.trim()) {
        Ok(Value::Array(items)) => items
            .into_iter()
            .filter_map(|v| serde_json::from_value(v).ok())
            .collect(),
        Ok(v) => serde_json::from_value(v).into_iter().collect(),
        Err(_) => Vec::new(),
    }
}

/// Incremental SSE decoder. Feed bytes, get complete event `data` payloads.
#[derive(Default)]
pub(crate) struct SseDecoder {
    line_buf: Vec<u8>,
    data: Vec<String>,
    event_bytes: usize,
}

impl SseDecoder {
    pub(crate) fn feed(&mut self, chunk: &[u8]) -> Result<Vec<String>> {
        let mut events = Vec::new();
        for &b in chunk {
            if b == b'\n' {
                let mut line = std::mem::take(&mut self.line_buf);
                if line.last() == Some(&b'\r') {
                    line.pop();
                }
                self.handle_line(&line, &mut events);
            } else {
                self.line_buf.push(b);
                self.event_bytes += 1;
                if self.event_bytes > MAX_MESSAGE_BYTES {
                    bail!("MCP SSE event exceeded {MAX_MESSAGE_BYTES} bytes");
                }
            }
        }
        Ok(events)
    }

    /// Flush a trailing event at end of stream.
    pub(crate) fn finish(&mut self) -> Vec<String> {
        let mut events = Vec::new();
        if !self.line_buf.is_empty() {
            let line = std::mem::take(&mut self.line_buf);
            self.handle_line(&line, &mut events);
        }
        self.handle_line(b"", &mut events);
        events
    }

    fn handle_line(&mut self, line: &[u8], events: &mut Vec<String>) {
        if line.is_empty() {
            if !self.data.is_empty() {
                events.push(self.data.join("\n"));
                self.data.clear();
            }
            self.event_bytes = 0;
            return;
        }
        let line = String::from_utf8_lossy(line);
        if let Some(rest) = line.strip_prefix("data:") {
            self.data
                .push(rest.strip_prefix(' ').unwrap_or(rest).to_string());
        }
    }
}

/// Read SSE events until the response for `id` arrives.
pub(crate) async fn read_sse_until<S, B>(mut stream: S, id: u64) -> Result<JsonRpcResponse>
where
    S: futures::Stream<Item = Result<B>> + Unpin,
    B: AsRef<[u8]>,
{
    let mut decoder = SseDecoder::default();
    let find = |events: Vec<String>| {
        events
            .iter()
            .flat_map(|e| parse_json_messages(e))
            .find(|r| r.id == Some(id))
    };
    while let Some(chunk) = stream.next().await {
        if let Some(r) = find(decoder.feed(chunk?.as_ref())?) {
            return Ok(r);
        }
    }
    find(decoder.finish()).context("SSE stream ended without a response")
}

/// Parse a complete SSE body (test helper).
#[cfg(test)]
pub(crate) fn parse_sse_messages(text: &str) -> Vec<JsonRpcResponse> {
    let mut d = SseDecoder::default();
    let mut events = d.feed(text.as_bytes()).unwrap_or_default();
    events.extend(d.finish());
    events.iter().flat_map(|e| parse_json_messages(e)).collect()
}

#[cfg(test)]
#[path = "http_tests.rs"]
mod tests;
