use super::*;
use crate::mcp::test_http_mock::{self, Recorded, Reply};
use std::sync::{Arc, Mutex};

fn cfg(url: &str, oauth: Option<McpOAuthConfig>) -> McpServerConfig {
    let mut c: McpServerConfig =
        serde_json::from_value(serde_json::json!({ "type": "http", "url": url })).unwrap();
    c.oauth = oauth;
    c
}

#[test]
fn figma_identity_only_for_exact_host() {
    let d = McpOAuthConfig::default();
    assert_eq!(
        effective_client_name("https://mcp.figma.com/mcp", &d),
        "Codex"
    );
    assert_eq!(
        effective_client_name("https://evil.figma.com/mcp", &d),
        "jcode"
    );
    assert_eq!(
        effective_client_name("https://mcp.figma.com.evil.io/mcp", &d),
        "jcode"
    );
    let o = McpOAuthConfig {
        client_name: Some("Custom".into()),
        ..Default::default()
    };
    assert_eq!(
        effective_client_name("https://mcp.figma.com/mcp", &o),
        "Custom"
    );
}

#[test]
fn pkce_s256_matches_rfc7636_vector() {
    assert_eq!(
        pkce_challenge("dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk"),
        "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM"
    );
}

#[test]
fn endpoint_validation() {
    assert!(validate_endpoint("https://a.com/x").is_ok());
    assert!(validate_endpoint("http://localhost:3/x").is_ok());
    assert!(validate_endpoint("http://a.com/x").is_err());
    assert!(validate_endpoint("https://u:p@a.com/x").is_err());
    assert!(validate_endpoint("https://a.com/x#f").is_err());
}

#[test]
fn resource_match_requires_segment_boundary() {
    let s = Url::parse("https://mcp.figma.com/mcp").unwrap();
    assert!(resource_matches("https://mcp.figma.com/mcp", &s));
    assert!(resource_matches("https://mcp.figma.com/", &s));
    assert!(!resource_matches("https://mcp.figma.com/mc", &s));
    assert!(!resource_matches("https://other.com/mcp", &s));
    let evil = Url::parse("https://mcp.figma.com/mcpevil").unwrap();
    assert!(!resource_matches("https://mcp.figma.com/mcp", &evil));
}

#[test]
fn auth_method_negotiation() {
    let figma = vec![
        "client_secret_basic".to_string(),
        "client_secret_post".to_string(),
    ];
    assert_eq!(
        choose_auth_method(&figma, false),
        TokenAuthMethod::ClientSecretPost
    );
    assert_eq!(
        choose_auth_method(&["none".into(), "client_secret_post".into()], false),
        TokenAuthMethod::None
    );
    assert_eq!(choose_auth_method(&[], false), TokenAuthMethod::None);
    assert_eq!(
        choose_auth_method(&[], true),
        TokenAuthMethod::ClientSecretBasic
    );
}

#[test]
fn parse_www_authenticate() {
    assert_eq!(
        parse_resource_metadata(r#"Bearer error="x", resource_metadata="https://a/b""#).as_deref(),
        Some("https://a/b")
    );
    assert_eq!(parse_resource_metadata("Bearer realm=x"), None);
}

#[test]
fn store_key_varies_with_url_and_client() {
    let d = McpOAuthConfig::default();
    let a = store_key("https://a.com/mcp", &d);
    assert_ne!(a, store_key("https://b.com/mcp", &d));
    let c = McpOAuthConfig {
        client_id: Some("x".into()),
        ..Default::default()
    };
    assert_ne!(a, store_key("https://a.com/mcp", &c));
}

#[test]
fn store_roundtrip_perms_and_corrupt_protection() {
    let dir = tempfile::tempdir().unwrap();
    let p = dir.path().join("mcp-oauth.json");
    let creds = StoredCredentials {
        access_token: "tok".into(),
        ..Default::default()
    };
    save_credentials(&p, "k", creds.clone()).unwrap();
    assert_eq!(load_credentials(&p, "k"), Some(creds.clone()));
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(&p).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600);
    }
    assert!(remove_credentials(&p, "k").unwrap());
    assert!(!remove_credentials(&p, "k").unwrap());
    std::fs::write(&p, "{not json").unwrap();
    assert!(save_credentials(&p, "k2", creds).is_err());
    assert_eq!(std::fs::read_to_string(&p).unwrap(), "{not json");
}

#[tokio::test]
async fn expired_token_is_refreshed_with_secret_post() {
    let token_calls = Arc::new(Mutex::new(Vec::<Recorded>::new()));
    let tc = Arc::clone(&token_calls);
    let server = test_http_mock::start(Arc::new(move |req| {
        tc.lock().unwrap().push(req.clone());
        Reply::json(
            200,
            r#"{"access_token":"new","expires_in":3600,"refresh_token":"r2"}"#,
        )
    }))
    .await;
    let dir = tempfile::tempdir().unwrap();
    let p = dir.path().join("s.json");
    save_credentials(
        &p,
        "k",
        StoredCredentials {
            client_id: "cid".into(),
            client_secret: Some("sec".into()),
            token_endpoint_auth_method: TokenAuthMethod::ClientSecretPost,
            token_endpoint: format!("{}/token", server.base),
            access_token: "old".into(),
            refresh_token: Some("r1".into()),
            expires_at: Some(1),
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(
        access_token_at(&p, "k").await.unwrap().as_deref(),
        Some("new")
    );
    let stored = load_credentials(&p, "k").unwrap();
    assert_eq!(stored.refresh_token.as_deref(), Some("r2"));
    let body = &token_calls.lock().unwrap()[0].body;
    assert!(body.contains("grant_type=refresh_token"));
    assert!(body.contains("client_secret=sec"));
}

/// Full native flow against a mock Figma-like server: PRM discovery, AS
/// metadata, DCR (confidential, as live Figma advertises), PKCE, loopback
/// callback with state + iss, token exchange, storage. No browser is opened.
#[tokio::test]
async fn full_flow_against_mock_figma_like_server() {
    let base_cell: Arc<Mutex<String>> = Arc::new(Mutex::new(String::new()));
    let bc = Arc::clone(&base_cell);
    let server = test_http_mock::start(Arc::new(move |req| {
        let base = bc.lock().unwrap().clone();
        let path = req.path.split('?').next().unwrap_or("");
        match path {
            "/.well-known/oauth-protected-resource/mcp" => Reply::json(
                200,
                serde_json::json!({"resource": format!("{base}/mcp"),
                    "authorization_servers":[base], "scopes_supported":["mcp:connect"]})
                .to_string(),
            ),
            "/.well-known/oauth-authorization-server" => Reply::json(
                200,
                serde_json::json!({"issuer": base,
                    "authorization_endpoint": format!("{base}/authorize"),
                    "token_endpoint": format!("{base}/token"),
                    "registration_endpoint": format!("{base}/register"),
                    "code_challenge_methods_supported":["S256"],
                    "token_endpoint_auth_methods_supported":["client_secret_basic","client_secret_post"]})
                .to_string(),
            ),
            "/register" => Reply::json(
                201,
                r#"{"client_id":"cid","client_secret":"csec","token_endpoint_auth_method":"client_secret_post"}"#,
            ),
            "/token" => Reply::json(
                200,
                r#"{"access_token":"at","refresh_token":"rt","expires_in":3600,"token_type":"Bearer"}"#,
            ),
            _ => Reply::json(404, "{}"),
        }
    }))
    .await;
    *base_cell.lock().unwrap() = server.base.clone();

    let dir = tempfile::tempdir().unwrap();
    let store = dir.path().join("mcp-oauth.json");
    let config = cfg(&format!("{}/mcp", server.base), None);
    let issuer = server.base.clone();
    let seen_url = Arc::new(Mutex::new(String::new()));
    let su = Arc::clone(&seen_url);
    authenticate_at(
        &store,
        "figma",
        &config,
        AuthOptions {
            open_browser: false,
        },
        move |url| {
            *su.lock().unwrap() = url.to_string();
            let u = Url::parse(url).unwrap();
            let q: HashMap<String, String> = u.query_pairs().into_owned().collect();
            let cb = format!(
                "{}?code=thecode&state={}&iss={}",
                q["redirect_uri"],
                q["state"],
                urlencoding::encode(&issuer)
            );
            tokio::spawn(async move {
                let _ = reqwest::get(cb).await;
            });
        },
    )
    .await
    .unwrap();

    let auth_url = Url::parse(&seen_url.lock().unwrap()).unwrap();
    let q: HashMap<String, String> = auth_url.query_pairs().into_owned().collect();
    assert_eq!(q["code_challenge_method"], "S256");
    assert_eq!(q["scope"], "mcp:connect");
    assert_eq!(q["resource"], format!("{}/mcp", server.base));
    assert!(q["redirect_uri"].starts_with("http://127.0.0.1:"));
    assert!(q["redirect_uri"].ends_with("/callback"));

    let reqs = server.requests.lock().unwrap().clone();
    let reg = reqs.iter().find(|r| r.path == "/register").unwrap();
    let reg_body: serde_json::Value = serde_json::from_str(&reg.body).unwrap();
    assert_eq!(reg_body["token_endpoint_auth_method"], "client_secret_post");
    let tok = reqs.iter().find(|r| r.path == "/token").unwrap();
    assert!(tok.body.contains("code=thecode"));
    assert!(tok.body.contains("client_secret=csec"));
    assert!(tok.body.contains("code_verifier="));

    let key = store_key(&config.url.clone().unwrap(), &McpOAuthConfig::default());
    let creds = load_credentials(&store, &key).unwrap();
    assert_eq!(creds.access_token, "at");
    assert_eq!(
        creds.token_endpoint_auth_method,
        TokenAuthMethod::ClientSecretPost
    );
}

#[tokio::test]
async fn callback_rejects_state_and_issuer_mismatch() {
    for (query, expect) in [
        ("code=c&state=wrong", "state mismatch"),
        ("code=c&state=s&iss=https%3A%2F%2Fevil", "issuer mismatch"),
    ] {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let url = format!("http://127.0.0.1:{port}/callback?{query}");
        tokio::spawn(async move {
            let _ = reqwest::get(url).await;
        });
        let err = wait_for_callback(listener, "s", Some("https://good"))
            .await
            .unwrap_err();
        assert!(err.to_string().contains(expect), "{err}");
    }
}

#[tokio::test]
async fn metadata_for_other_resource_is_rejected() {
    let server = test_http_mock::start(Arc::new(|req| {
        if req.path.starts_with("/.well-known/oauth-protected-resource") {
            Reply::json(200, r#"{"resource":"https://attacker.example/mcp","authorization_servers":["https://attacker.example"]}"#)
        } else {
            Reply::json(404, "{}")
        }
    }))
    .await;
    let client = http_client().unwrap();
    let err = discover(&client, &format!("{}/mcp", server.base), None)
        .await
        .err()
        .unwrap();
    assert!(err.to_string().contains("is for"), "{err}");
}

/// Logout during an in-flight refresh must not resurrect credentials.
#[tokio::test]
async fn refresh_does_not_resurrect_after_logout() {
    let dir = tempfile::tempdir().unwrap();
    let p = dir.path().join("s.json");
    let p2 = p.clone();
    let server = test_http_mock::start(Arc::new(move |_| {
        // Logout happens while the token endpoint is "processing".
        remove_credentials(&p2, "k").unwrap();
        Reply::json(200, r#"{"access_token":"new","expires_in":3600}"#)
    }))
    .await;
    save_credentials(
        &p,
        "k",
        StoredCredentials {
            client_id: "cid".into(),
            token_endpoint: format!("{}/token", server.base),
            access_token: "old".into(),
            refresh_token: Some("r1".into()),
            expires_at: Some(1),
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(access_token_at(&p, "k").await.unwrap(), None);
    assert_eq!(load_credentials(&p, "k"), None);
}

/// LIVE, opt-in: proves Figma's client_name allowlist accepts jcode's
/// compatibility identity. Run explicitly with:
///   JCODE_LIVE_FIGMA_DCR=1 cargo test -p jcode-base live_figma_discovery_and_dcr -- --ignored
///
/// This performs real network requests to mcp.figma.com / api.figma.com:
/// metadata discovery plus ONE unauthenticated dynamic client registration.
/// It never opens a browser, never requests user authorization, never obtains
/// a token, and grants no account access. The registered client is an
/// orphaned, unused public registration. client_id/client_secret are never
/// printed.
#[tokio::test]
#[ignore = "live network: registers an unauthenticated OAuth client with Figma"]
async fn live_figma_discovery_and_dcr() {
    if std::env::var("JCODE_LIVE_FIGMA_DCR").as_deref() != Ok("1") {
        eprintln!("set JCODE_LIVE_FIGMA_DCR=1 to run");
        return;
    }
    let url = "https://mcp.figma.com/mcp";
    let client = http_client().unwrap();
    let challenge = probe_challenge(&client, url).await;
    let flow = discover(&client, url, challenge.as_deref()).await.unwrap();
    assert_eq!(flow.resource.as_deref(), Some(url));

    let name = effective_client_name(url, &McpOAuthConfig::default());
    assert_eq!(name, FIGMA_COMPAT_CLIENT_NAME);
    let method = choose_auth_method(&flow.as_meta.token_endpoint_auth_methods_supported, false);
    let reg_endpoint = flow
        .as_meta
        .registration_endpoint
        .as_deref()
        .expect("Figma advertises a registration endpoint");
    // Redirect target is never contacted: no authorization happens.
    let registration = register_client(
        &client,
        reg_endpoint,
        &name,
        "http://127.0.0.1:43219/callback",
        method,
    )
    .await
    .expect("Figma accepted client_name registration");
    assert!(!registration.client_id.is_empty());
    let supported = &flow.as_meta.token_endpoint_auth_methods_supported;
    assert!(
        supported.is_empty() || supported.iter().any(|m| m == registration.auth_method.as_str()),
        "registered method {} not in advertised {:?}",
        registration.auth_method.as_str(),
        supported
    );
    if registration.auth_method != TokenAuthMethod::None {
        assert!(registration.client_secret.is_some());
    }
    eprintln!(
        "live Figma DCR ok: client_name={name} method={} secret_issued={}",
        registration.auth_method.as_str(),
        registration.client_secret.is_some()
    );
}
