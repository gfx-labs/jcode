//! End-to-end OAuth authorization code, dynamic registration, PKCE and refresh
//! against a loopback authorization server. No browser or external network.
use base64::Engine;
use jcode_base::mcp::{
    McpClient, McpServerConfig,
    oauth::{self, McpAuthStatus},
};
use serde_json::{Value, json};
use sha2::Digest;
use std::sync::{Arc, Mutex};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpListener,
};

#[tokio::test]
async fn oauth_dcr_pkce_callback_persistence_and_csrf() {
    let home = tempfile::tempdir().unwrap();
    unsafe {
        std::env::set_var("JCODE_HOME", home.path());
        std::env::set_var("JCODE_MCP_OAUTH_NO_BROWSER", "1");
    }
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let origin = format!("http://{}", listener.local_addr().unwrap());
    let events = Arc::new(Mutex::new(Vec::<(String, String)>::new()));
    let records = events.clone();
    let bearer_observed = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let server_bearer = bearer_observed.clone();
    let challenge = Arc::new(Mutex::new(None::<String>));
    let server_challenge = challenge.clone();
    let base = origin.clone();
    let server = tokio::spawn(async move {
        loop {
            let Ok((mut stream, _)) = listener.accept().await else {
                break;
            };
            let base = base.clone();
            let records = records.clone();
            let challenge = server_challenge.clone();
            let bearer = server_bearer.clone();
            tokio::spawn(async move {
                let mut request = Vec::new();
                let end = loop {
                    let mut chunk = [0u8; 4096];
                    let Ok(n) = stream.read(&mut chunk).await else {
                        return;
                    };
                    if n == 0 {
                        return;
                    }
                    request.extend_from_slice(&chunk[..n]);
                    if let Some(at) = request.windows(4).position(|b| b == b"\r\n\r\n") {
                        break at + 4;
                    }
                };
                let headers = String::from_utf8_lossy(&request[..end]).to_string();
                let len = headers
                    .lines()
                    .find_map(|line| {
                        let lower = line.to_ascii_lowercase();
                        lower
                            .strip_prefix("content-length: ")
                            .and_then(|v| v.trim().parse::<usize>().ok())
                    })
                    .unwrap_or(0);
                while request.len() - end < len {
                    let mut chunk = [0u8; 4096];
                    let Ok(n) = stream.read(&mut chunk).await else {
                        return;
                    };
                    if n == 0 {
                        return;
                    }
                    request.extend_from_slice(&chunk[..n]);
                }
                let path = headers
                    .lines()
                    .next()
                    .unwrap()
                    .split_whitespace()
                    .nth(1)
                    .unwrap()
                    .to_owned();
                let body = String::from_utf8_lossy(&request[end..end + len]).to_string();
                records.lock().unwrap().push((path.clone(), body.clone()));
                let (status, extra, response) = match path.as_str() {
                    "/mcp" if headers.to_ascii_lowercase().contains("authorization: bearer renewed-token") => {
                        bearer.store(true, std::sync::atomic::Ordering::SeqCst);
                        let message = serde_json::from_str::<Value>(&body).unwrap();
                        let id = message["id"].clone();
                        match message["method"].as_str().unwrap_or("") {
                            "initialize" => ("200 OK", String::new(), json!({"jsonrpc":"2.0","id":id,"result":{"protocolVersion":"2025-03-26","capabilities":{"tools":{}},"serverInfo":{"name":"fixture","version":"1"}}}).to_string()),
                            "notifications/initialized" => ("202 Accepted", String::new(), String::new()),
                            "tools/list" => ("200 OK", String::new(), json!({"jsonrpc":"2.0","id":id,"result":{"tools":[]}}).to_string()),
                            _ => ("404 Not Found", String::new(), String::new()),
                        }
                    },
                    "/mcp" => ("401 Unauthorized", format!("WWW-Authenticate: Bearer resource_metadata=\"{base}/.well-known/oauth-protected-resource\"\r\n"), String::new()),
                    "/.well-known/oauth-protected-resource" => ("200 OK", String::new(), json!({"resource":format!("{base}/mcp"),"authorization_servers":[base],"scopes_supported":["read"]}).to_string()),
                    "/.well-known/oauth-authorization-server" => ("200 OK", String::new(), json!({"issuer":base,"authorization_endpoint":format!("{base}/authorize"),"token_endpoint":format!("{base}/token"),"registration_endpoint":format!("{base}/register"),"code_challenge_methods_supported":["S256"],"token_endpoint_auth_methods_supported":["client_secret_post","client_secret_basic"]}).to_string()),
                    "/register" => ("201 Created", String::new(), json!({"client_id":"registered-client","client_secret":"registered-secret"}).to_string()),
                    "/token" => {
                        let params: std::collections::HashMap<_,_> = url::form_urlencoded::parse(body.as_bytes()).into_owned().collect();
                        assert_eq!(params.get("client_id").map(String::as_str), Some("registered-client"));
                        assert_eq!(params.get("client_secret").map(String::as_str), Some("registered-secret"));
                        if params.get("grant_type").map(String::as_str) == Some("authorization_code") {
                            assert_eq!(params.get("code").map(String::as_str), Some("test-code"));
                            let challenge = challenge.lock().unwrap().clone().expect("authorization challenge");
                            let actual = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(sha2::Sha256::digest(params["code_verifier"].as_bytes()));
                            assert_eq!(actual, challenge, "PKCE verifier does not match authorization challenge");
                            ("200 OK", String::new(), json!({"access_token":"first-token","refresh_token":"renew-token","expires_in":1}).to_string())
                        } else {
                            assert_eq!(params.get("grant_type").map(String::as_str), Some("refresh_token"));
                            assert_eq!(params.get("refresh_token").map(String::as_str), Some("renew-token"));
                            ("200 OK", String::new(), json!({"access_token":"renewed-token","refresh_token":"renew-token","expires_in":3600}).to_string())
                        }
                    },
                    _ => ("404 Not Found", String::new(), String::new()),
                };
                let response = format!(
                    "HTTP/1.1 {status}\r\nContent-Type: application/json\r\n{extra}Content-Length: {}\r\nConnection: close\r\n\r\n{response}",
                    response.len()
                );
                let _ = stream.write_all(response.as_bytes()).await;
            });
        }
    });
    let config: McpServerConfig =
        serde_json::from_value(json!({"type":"http","url":format!("{origin}/mcp")})).unwrap();
    let auth_url = Arc::new(Mutex::new(None::<String>));
    let captured = auth_url.clone();
    let callback_challenge = challenge.clone();
    tokio::time::timeout(
        std::time::Duration::from_secs(8),
        oauth::authenticate("fixture", &config, move |url| {
            *captured.lock().unwrap() = Some(url.to_owned());
            let url: url::Url = url.parse().unwrap();
            let query: std::collections::HashMap<_, _> = url.query_pairs().into_owned().collect();
            assert_eq!(
                query.get("code_challenge_method").map(String::as_str),
                Some("S256")
            );
            assert_eq!(
                query.get("client_id").map(String::as_str),
                Some("registered-client")
            );
            *callback_challenge.lock().unwrap() = Some(query["code_challenge"].clone());
            let callback = format!(
                "{}?code=test-code&state={}",
                query["redirect_uri"], query["state"]
            );
            tokio::spawn(async move {
                reqwest::get(callback).await.unwrap();
            });
        }),
    )
    .await
    .unwrap()
    .unwrap();
    assert!(auth_url.lock().unwrap().is_some());
    assert!(matches!(
        oauth::auth_status(&config),
        McpAuthStatus::Expired { refreshable: true } | McpAuthStatus::Authenticated
    ));
    let registration = events
        .lock()
        .unwrap()
        .iter()
        .find(|(p, _)| p == "/register")
        .unwrap()
        .1
        .clone();
    let registration: Value = serde_json::from_str(&registration).unwrap();
    assert_eq!(registration["client_name"], "jcode");
    assert_eq!(
        registration["token_endpoint_auth_method"],
        "client_secret_post"
    );
    let store = home.path().join("mcp-oauth.json");
    assert!(store.exists(), "OAuth credentials not persisted");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            std::fs::metadata(&store).unwrap().permissions().mode() & 0o077,
            0,
            "OAuth credentials must be owner-only"
        );
    }
    // A one-second token is expired by the 60-second refresh margin. Connecting
    // through the public transport must refresh before initialize and send only
    // the renewed bearer to the MCP endpoint.
    let client = tokio::time::timeout(
        std::time::Duration::from_secs(8),
        McpClient::connect("fixture".into(), &config),
    )
    .await
    .unwrap();
    let mut client = client.expect("refresh should authenticate initialize and list tools");
    assert!(client.tools().is_empty());
    assert!(
        bearer_observed.load(std::sync::atomic::Ordering::SeqCst),
        "renewed bearer absent from MCP request"
    );
    client.shutdown().await;
    assert!(
        events
            .lock()
            .unwrap()
            .iter()
            .any(|(p, b)| p == "/token" && b.contains("grant_type=refresh_token"))
    );
    assert!(
        events
            .lock()
            .unwrap()
            .iter()
            .any(|(p, b)| p == "/mcp" && b.contains("initialize"))
    );
    assert!(matches!(
        oauth::auth_status(&config),
        McpAuthStatus::Authenticated
    ));
    assert!(oauth::logout(&config).unwrap());
    assert_eq!(oauth::auth_status(&config), McpAuthStatus::NotAuthenticated);
    // A callback with a mismatched state must not exchange a code or persist tokens.
    let prior_tokens = events
        .lock()
        .unwrap()
        .iter()
        .filter(|(p, _)| p == "/token")
        .count();
    let error = tokio::time::timeout(
        std::time::Duration::from_secs(8),
        oauth::authenticate("fixture", &config, |url| {
            let parsed: url::Url = url.parse().unwrap();
            let query: std::collections::HashMap<_, _> =
                parsed.query_pairs().into_owned().collect();
            let callback = format!("{}?code=bad-code&state=wrong-state", query["redirect_uri"]);
            tokio::spawn(async move {
                reqwest::get(callback).await.unwrap();
            });
        }),
    )
    .await
    .unwrap()
    .unwrap_err();
    assert!(error.to_string().contains("state mismatch"), "{error:#}");
    assert_eq!(
        events
            .lock()
            .unwrap()
            .iter()
            .filter(|(p, _)| p == "/token")
            .count(),
        prior_tokens
    );
    assert_eq!(oauth::auth_status(&config), McpAuthStatus::NotAuthenticated);
    server.abort();
}
