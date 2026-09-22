//! Native OAuth 2.1 support for remote (HTTP) MCP servers.
//!
//! Implements the MCP authorization spec without any Node proxy:
//! - Protected resource metadata discovery (RFC 9728), honoring the
//!   `resource_metadata` parameter of a `WWW-Authenticate` challenge.
//! - Authorization server metadata discovery (RFC 8414, then OIDC), with a
//!   legacy fallback to the MCP server origin.
//! - Dynamic client registration (RFC 7591) when no `client_id` is configured.
//! - Authorization code + PKCE S256 through the system browser and a loopback
//!   callback on `127.0.0.1` with an ephemeral port (or `callback_port`).
//! - `state` validation, resource indicators (RFC 8707), and refresh tokens.
//!
//! Credentials are stored in `~/.jcode/mcp-oauth.json` (mode 0600), keyed by
//! the server URL *and* the client configuration, never by server name alone,
//! so renaming a server or swapping its URL cannot leak a token to another
//! endpoint.
//!
//! # Figma compatibility identity
//!
//! Figma's MCP server (`https://mcp.figma.com/mcp`) only accepts dynamic client
//! registrations whose `client_name` is on an allowlist of known MCP clients.
//! The community workaround referenced by
//! <https://jaketrent.com/post/enable-figma-mcp-opencode/> (opencode commit
//! `cbc7df7`) registers as `client_name: "Codex"` with no `client_uri` and a
//! `/callback` redirect path. The token endpoint auth method is negotiated from
//! metadata (live Figma advertises only `client_secret_basic` and
//! `client_secret_post`, so jcode registers confidentially and uses the
//! issued secret). For
//! the exact host `mcp.figma.com` jcode therefore defaults to that
//! compatibility identity. Every other server registers as `jcode`, and
//! `oauth.client_name` overrides the default for any server.

use super::protocol::McpServerConfig;
use anyhow::{Context, Result, bail};
use base64::Engine;
use rand::RngCore;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex as StdMutex;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use url::Url;

/// Host that receives the Figma compatibility client identity.
pub const FIGMA_MCP_HOST: &str = "mcp.figma.com";
/// Client name Figma's registration allowlist accepts (see module docs).
pub const FIGMA_COMPAT_CLIENT_NAME: &str = "Codex";
/// Default client name for every other server.
pub const DEFAULT_CLIENT_NAME: &str = "jcode";
const CALLBACK_PATH: &str = "/callback";
const AUTH_TIMEOUT: Duration = Duration::from_secs(300);
const AUTH_REQUIRED_MARKER: &str = "requires OAuth authentication";

/// Per-server OAuth configuration (`oauth` key of an MCP server entry).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct McpOAuthConfig {
    /// Pre-registered client id. Skips dynamic registration when set.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub client_id: Option<String>,
    /// Client secret for confidential pre-registered clients.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub client_secret: Option<String>,
    /// `client_name` for dynamic registration (defaults described in module docs).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub client_name: Option<String>,
    /// Scopes to request. Empty uses the server's advertised scopes.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub scopes: Vec<String>,
    /// Fixed loopback callback port. `None` picks an ephemeral port.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub callback_port: Option<u16>,
}

/// Authentication state shown in `/mcp` listings.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum McpAuthStatus {
    /// Not an HTTP server, or it uses a static Authorization header.
    NotApplicable,
    NotAuthenticated,
    Authenticated,
    /// Access token expired. `refreshable` means it will renew automatically.
    Expired { refreshable: bool },
}

impl McpAuthStatus {
    pub fn label(&self) -> &'static str {
        match self {
            Self::NotApplicable => "n/a",
            Self::NotAuthenticated => "not authenticated",
            Self::Authenticated => "authenticated",
            Self::Expired { refreshable: true } => "authenticated (refresh pending)",
            Self::Expired { refreshable: false } => "expired",
        }
    }
}

/// Client authentication at the token endpoint.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TokenAuthMethod {
    /// Public client (PKCE only).
    #[default]
    None,
    ClientSecretPost,
    ClientSecretBasic,
}

impl TokenAuthMethod {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::None => "none",
            Self::ClientSecretPost => "client_secret_post",
            Self::ClientSecretBasic => "client_secret_basic",
        }
    }

    fn parse(s: &str) -> Option<Self> {
        match s {
            "none" => Some(Self::None),
            "client_secret_post" => Some(Self::ClientSecretPost),
            "client_secret_basic" => Some(Self::ClientSecretBasic),
            _ => None,
        }
    }
}

/// Pick the token endpoint auth method from server metadata. Public clients
/// prefer `none`; servers that only list secret methods (live Figma lists
/// `client_secret_basic`/`client_secret_post`) get a confidential
/// registration. An empty list means the RFC 8414 default,
/// `client_secret_basic`, but public registration is still attempted first
/// when we hold no secret since DCR servers typically accept it.
pub fn choose_auth_method(supported: &[String], have_secret: bool) -> TokenAuthMethod {
    let has = |m: &str| supported.iter().any(|s| s == m);
    if supported.is_empty() {
        return if have_secret {
            TokenAuthMethod::ClientSecretBasic
        } else {
            TokenAuthMethod::None
        };
    }
    if !have_secret && has("none") {
        return TokenAuthMethod::None;
    }
    if has("client_secret_post") {
        TokenAuthMethod::ClientSecretPost
    } else if has("client_secret_basic") {
        TokenAuthMethod::ClientSecretBasic
    } else {
        TokenAuthMethod::None
    }
}

/// Client identity used for token requests.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Registration {
    pub client_id: String,
    pub client_secret: Option<String>,
    pub auth_method: TokenAuthMethod,
}

/// Credentials persisted for one server URL + client configuration.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct StoredCredentials {
    pub server_url: String,
    pub client_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub client_secret: Option<String>,
    pub token_endpoint: String,
    pub token_endpoint_auth_method: TokenAuthMethod,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub resource: Option<String>,
    pub access_token: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub refresh_token: Option<String>,
    /// Unix seconds. `None` means the server gave no expiry.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub expires_at: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scope: Option<String>,
}

impl StoredCredentials {
    fn is_expired(&self, now: u64) -> bool {
        // Treat tokens within 60s of expiry as expired to avoid mid-request failure.
        self.expires_at.is_some_and(|exp| exp <= now + 60)
    }
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Run the interactive OAuth flow for `config`.
///
/// Calls `on_url` with the authorization URL as soon as it is ready (the
/// browser is also opened best-effort), then waits up to five minutes for the
/// loopback callback. Credentials are stored on success; reconnect the server
/// afterwards to use them.
pub async fn authenticate<F>(server_name: &str, config: &McpServerConfig, on_url: F) -> Result<()>
where
    F: Fn(&str) + Send + Sync,
{
    let open_browser = std::env::var("JCODE_MCP_OAUTH_NO_BROWSER").map_or(true, |v| v != "1");
    authenticate_with(server_name, config, AuthOptions { open_browser }, on_url).await
}

/// Options for [`authenticate_with`].
#[derive(Debug, Clone, Copy)]
pub struct AuthOptions {
    /// Open the system browser. When false only `on_url` receives the URL
    /// (headless sessions, integration tests).
    pub open_browser: bool,
}

impl Default for AuthOptions {
    fn default() -> Self {
        Self { open_browser: true }
    }
}

/// [`authenticate`] with explicit options.
pub async fn authenticate_with<F>(
    server_name: &str,
    config: &McpServerConfig,
    options: AuthOptions,
    on_url: F,
) -> Result<()>
where
    F: Fn(&str) + Send + Sync,
{
    authenticate_at(&default_store_path()?, server_name, config, options, on_url).await
}

pub(crate) async fn authenticate_at<F>(
    store_path: &Path,
    server_name: &str,
    config: &McpServerConfig,
    options: AuthOptions,
    on_url: F,
) -> Result<()>
where
    F: Fn(&str) + Send + Sync,
{
    let server_url = server_url(config)?;
    let oauth_cfg = oauth_config(config);
    let client = http_client()?;
    let challenge = remembered_challenge(&server_url);
    let flow = prepare_flow(&client, &server_url, challenge.as_deref()).await?;

    let listener = tokio::net::TcpListener::bind(("127.0.0.1", oauth_cfg.callback_port.unwrap_or(0)))
        .await
        .context("Failed to bind OAuth loopback callback listener")?;
    let port = listener.local_addr()?.port();
    let redirect_uri = format!("http://127.0.0.1:{port}{CALLBACK_PATH}");

    let registration = match (&oauth_cfg.client_id, &flow.as_meta.registration_endpoint) {
        (Some(id), _) => Registration {
            client_id: id.clone(),
            auth_method: if oauth_cfg.client_secret.is_some() {
                choose_auth_method(&flow.as_meta.token_endpoint_auth_methods_supported, true)
            } else {
                TokenAuthMethod::None
            },
            client_secret: oauth_cfg.client_secret.clone(),
        },
        (None, Some(reg)) => {
            let name = effective_client_name(&server_url, &oauth_cfg);
            let method =
                choose_auth_method(&flow.as_meta.token_endpoint_auth_methods_supported, false);
            register_client(&client, reg, &name, &redirect_uri, method).await?
        }
        (None, None) => bail!(
            "MCP server '{server_name}' does not support dynamic client registration; set oauth.client_id"
        ),
    };

    let pkce = Pkce::generate();
    let state = random_token(32);
    let scope = requested_scope(&oauth_cfg, &flow);
    let auth_url = build_authorize_url(
        &flow.as_meta.authorization_endpoint,
        &registration.client_id,
        &redirect_uri,
        &state,
        &pkce.challenge,
        scope.as_deref(),
        flow.resource.as_deref(),
    )?;

    on_url(auth_url.as_str());
    if options.open_browser {
        let _ = open::that_detached(auth_url.as_str());
    }

    let expected_iss = flow.as_meta.issuer.clone();
    let code = tokio::time::timeout(
        AUTH_TIMEOUT,
        wait_for_callback(listener, &state, expected_iss.as_deref()),
    )
    .await
    .context("Timed out waiting for OAuth browser callback")??;

    let token = exchange_code(
        &client,
        &flow.as_meta.token_endpoint,
        &registration,
        &code,
        &redirect_uri,
        &pkce.verifier,
        flow.resource.as_deref(),
    )
    .await?;

    let creds = StoredCredentials {
        server_url: server_url.clone(),
        client_id: registration.client_id,
        client_secret: registration.client_secret,
        token_endpoint_auth_method: registration.auth_method,
        token_endpoint: flow.as_meta.token_endpoint.clone(),
        resource: flow.resource.clone(),
        access_token: token.access_token,
        refresh_token: token.refresh_token,
        expires_at: token.expires_in.map(|s| now_secs() + s),
        scope: token.scope.or(scope),
    };
    save_credentials(store_path, &store_key(&server_url, &oauth_cfg), creds)?;
    crate::logging::info(&format!("MCP: OAuth completed for '{server_name}'"));
    Ok(())
}

/// Current authentication state for display.
pub fn auth_status(config: &McpServerConfig) -> McpAuthStatus {
    if !super::http::is_http_config(config) || has_static_auth(config) {
        return McpAuthStatus::NotApplicable;
    }
    let Ok(url) = server_url(config) else {
        return McpAuthStatus::NotApplicable;
    };
    let Ok(path) = default_store_path() else {
        return McpAuthStatus::NotAuthenticated;
    };
    match load_credentials(&path, &store_key(&url, &oauth_config(config))) {
        None => McpAuthStatus::NotAuthenticated,
        Some(c) if c.is_expired(now_secs()) => McpAuthStatus::Expired {
            refreshable: c.refresh_token.is_some(),
        },
        Some(_) => McpAuthStatus::Authenticated,
    }
}

/// Remove stored credentials. Returns whether anything was removed.
pub fn logout(config: &McpServerConfig) -> Result<bool> {
    let url = server_url(config)?;
    let path = default_store_path()?;
    remove_credentials(&path, &store_key(&url, &oauth_config(config)))
}

/// Whether an error came from a server demanding OAuth authentication.
pub fn is_auth_required_error(err: &anyhow::Error) -> bool {
    err.chain()
        .any(|e| is_auth_required_message(&e.to_string()))
}

pub(crate) fn is_auth_required_message(message: &str) -> bool {
    message.contains(AUTH_REQUIRED_MARKER)
}

pub(crate) fn auth_required_message(server_name: &str) -> String {
    format!(
        "MCP server '{server_name}' {AUTH_REQUIRED_MARKER}: run `/mcp auth {server_name}`"
    )
}

/// Bearer token for a request, refreshing it when expired. `Ok(None)` means no
/// credentials are stored (the request goes out unauthenticated).
pub(crate) async fn access_token(config: &McpServerConfig) -> Result<Option<String>> {
    let url = server_url(config)?;
    let path = default_store_path()?;
    let key = store_key(&url, &oauth_config(config));
    access_token_at(&path, &key).await
}

/// Record the `WWW-Authenticate` challenge from a 401 so a following
/// `authenticate` call can use its `resource_metadata` hint.
pub(crate) fn remember_challenge(config: &McpServerConfig, header: &str) {
    if let (Ok(url), Some(meta)) = (server_url(config), parse_resource_metadata(header))
        && let Ok(mut map) = CHALLENGES.lock()
    {
        map.get_or_insert_with(HashMap::new).insert(url, meta);
    }
}

/// Reject non-HTTPS endpoints unless they are loopback, and URLs carrying
/// credentials or fragments.
pub fn validate_endpoint(raw: &str) -> Result<Url> {
    let url = Url::parse(raw).with_context(|| format!("Invalid URL: {raw}"))?;
    if !url.username().is_empty() || url.password().is_some() {
        bail!("URL must not embed credentials: {raw}");
    }
    if url.fragment().is_some() {
        bail!("URL must not contain a fragment: {raw}");
    }
    match url.scheme() {
        "https" => Ok(url),
        "http" if is_loopback_host(&url) => Ok(url),
        _ => bail!("Refusing insecure (non-HTTPS, non-loopback) endpoint: {raw}"),
    }
}

// ---------------------------------------------------------------------------
// Internals
// ---------------------------------------------------------------------------

static CHALLENGES: StdMutex<Option<HashMap<String, String>>> = StdMutex::new(None);
// Serializes refreshes so concurrent requests do not burn a rotating refresh token.
static REFRESH_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

fn remembered_challenge(url: &str) -> Option<String> {
    CHALLENGES.lock().ok()?.as_ref()?.get(url).cloned()
}

fn is_loopback_host(url: &Url) -> bool {
    match url.host() {
        Some(url::Host::Domain(d)) => d.eq_ignore_ascii_case("localhost"),
        Some(url::Host::Ipv4(ip)) => ip.is_loopback(),
        Some(url::Host::Ipv6(ip)) => ip.is_loopback(),
        None => false,
    }
}

fn server_url(config: &McpServerConfig) -> Result<String> {
    let url = config
        .url
        .as_deref()
        .map(str::trim)
        .filter(|u| !u.is_empty())
        .context("MCP server has no url; OAuth only applies to HTTP servers")?;
    validate_endpoint(url)?;
    Ok(url.to_string())
}

fn oauth_config(config: &McpServerConfig) -> McpOAuthConfig {
    config.oauth.clone().unwrap_or_default()
}

fn has_static_auth(config: &McpServerConfig) -> bool {
    config
        .headers
        .keys()
        .any(|k| k.eq_ignore_ascii_case("authorization"))
}

/// Client name used for dynamic registration.
pub fn effective_client_name(server_url: &str, cfg: &McpOAuthConfig) -> String {
    if let Some(name) = cfg.client_name.as_deref().filter(|n| !n.trim().is_empty()) {
        return name.to_string();
    }
    let is_figma = Url::parse(server_url)
        .ok()
        .and_then(|u| u.host_str().map(|h| h.eq_ignore_ascii_case(FIGMA_MCP_HOST)))
        .unwrap_or(false);
    if is_figma {
        FIGMA_COMPAT_CLIENT_NAME.to_string()
    } else {
        DEFAULT_CLIENT_NAME.to_string()
    }
}

/// Storage key: server URL plus the client identity that obtained the token.
pub(crate) fn store_key(server_url: &str, cfg: &McpOAuthConfig) -> String {
    let mut h = Sha256::new();
    for part in [
        server_url,
        cfg.client_id.as_deref().unwrap_or(""),
        &effective_client_name(server_url, cfg),
        &cfg.scopes.join(" "),
    ] {
        h.update(part.as_bytes());
        h.update([0u8]);
    }
    hex_lower(&h.finalize())
}

fn hex_lower(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn random_token(bytes: usize) -> String {
    let mut buf = vec![0u8; bytes];
    rand::rng().fill_bytes(&mut buf);
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(buf)
}

pub(crate) struct Pkce {
    pub verifier: String,
    pub challenge: String,
}

impl Pkce {
    pub(crate) fn generate() -> Self {
        let verifier = random_token(48);
        let challenge = pkce_challenge(&verifier);
        Self { verifier, challenge }
    }
}

pub(crate) fn pkce_challenge(verifier: &str) -> String {
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()))
}

fn http_client() -> Result<reqwest::Client> {
    Ok(reqwest::Client::builder()
        // Redirects could send codes/secrets to an unvalidated origin.
        .redirect(reqwest::redirect::Policy::none())
        .timeout(Duration::from_secs(30))
        .build()?)
}

/// Extract `resource_metadata="..."` from a `WWW-Authenticate` header.
pub(crate) fn parse_resource_metadata(header: &str) -> Option<String> {
    let idx = header.find("resource_metadata=")?;
    let rest = &header[idx + "resource_metadata=".len()..];
    let value = if let Some(stripped) = rest.strip_prefix('"') {
        stripped.split('"').next()?
    } else {
        rest.split([',', ' ']).next()?
    };
    Some(value.to_string()).filter(|v| !v.is_empty())
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub(crate) struct ResourceMetadata {
    pub resource: Option<String>,
    pub authorization_servers: Vec<String>,
    pub scopes_supported: Vec<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub(crate) struct AuthServerMetadata {
    pub issuer: Option<String>,
    pub authorization_endpoint: String,
    pub token_endpoint: String,
    pub registration_endpoint: Option<String>,
    pub scopes_supported: Vec<String>,
    pub code_challenge_methods_supported: Vec<String>,
    pub token_endpoint_auth_methods_supported: Vec<String>,
}

pub(crate) struct Flow {
    pub as_meta: AuthServerMetadata,
    pub resource: Option<String>,
    pub scopes_supported: Vec<String>,
}

fn origin(url: &Url) -> String {
    url.origin().ascii_serialization()
}

fn well_known_urls(base: &Url, suffix: &str) -> Vec<String> {
    let path = base.path().trim_end_matches('/');
    let mut out = Vec::new();
    if !path.is_empty() {
        out.push(format!("{}/.well-known/{suffix}{path}", origin(base)));
    }
    out.push(format!("{}/.well-known/{suffix}", origin(base)));
    out
}

async fn get_json<T: for<'de> Deserialize<'de>>(client: &reqwest::Client, url: &str) -> Option<T> {
    validate_endpoint(url).ok()?;
    let resp = client
        .get(url)
        .header("accept", "application/json")
        .header("MCP-Protocol-Version", "2025-06-18")
        .send()
        .await
        .ok()?;
    if !resp.status().is_success() {
        return None;
    }
    resp.json().await.ok()
}

/// Does the metadata's `resource` identify this server (same origin, path prefix)?
fn resource_matches(resource: &str, server: &Url) -> bool {
    let Ok(res) = Url::parse(resource) else {
        return false;
    };
    if origin(&res) != origin(server) {
        return false;
    }
    let rp = res.path().trim_end_matches('/');
    let sp = server.path().trim_end_matches('/');
    rp.is_empty() || sp == rp || sp.strip_prefix(rp).is_some_and(|rest| rest.starts_with('/'))
}

pub(crate) async fn discover(
    client: &reqwest::Client,
    server_url: &str,
    challenge_metadata: Option<&str>,
) -> Result<Flow> {
    let server = validate_endpoint(server_url)?;

    let mut candidates = Vec::new();
    if let Some(hint) = challenge_metadata {
        candidates.push(hint.to_string());
    }
    candidates.extend(well_known_urls(&server, "oauth-protected-resource"));

    let mut prm: Option<ResourceMetadata> = None;
    for candidate in candidates {
        if let Some(meta) = get_json::<ResourceMetadata>(client, &candidate).await {
            if let Some(res) = meta.resource.as_deref()
                && !resource_matches(res, &server)
            {
                bail!(
                    "Protected resource metadata at {candidate} is for {res}, not {server_url}"
                );
            }
            prm = Some(meta);
            break;
        }
    }

    let (issuer, resource, scopes) = match &prm {
        Some(meta) => (
            meta.authorization_servers
                .first()
                .cloned()
                .unwrap_or_else(|| origin(&server)),
            Some(meta.resource.clone().unwrap_or_else(|| server_url.to_string())),
            meta.scopes_supported.clone(),
        ),
        // Legacy (2025-03-26) servers: the MCP origin is the auth server.
        None => (origin(&server), None, Vec::new()),
    };
    let issuer_url = validate_endpoint(&issuer).context("Invalid authorization server")?;

    let mut as_candidates = well_known_urls(&issuer_url, "oauth-authorization-server");
    as_candidates.extend(well_known_urls(&issuer_url, "openid-configuration"));
    let path = issuer_url.path().trim_end_matches('/');
    if !path.is_empty() {
        as_candidates.push(format!("{}{path}/.well-known/openid-configuration", origin(&issuer_url)));
    }

    let mut as_meta = None;
    for candidate in as_candidates {
        if let Some(meta) = get_json::<AuthServerMetadata>(client, &candidate).await
            && !meta.authorization_endpoint.is_empty()
            && !meta.token_endpoint.is_empty()
        {
            // RFC 8414 section 3.3: issuer must match the identifier used.
            if let Some(iss) = meta.issuer.as_deref()
                && iss.trim_end_matches('/') != issuer.trim_end_matches('/')
            {
                bail!("Authorization server metadata issuer {iss} does not match {issuer}");
            }
            as_meta = Some(meta);
            break;
        }
    }
    let as_meta = match as_meta {
        Some(m) => m,
        None if prm.is_none() => {
            let o = origin(&server);
            AuthServerMetadata {
                authorization_endpoint: format!("{o}/authorize"),
                token_endpoint: format!("{o}/token"),
                registration_endpoint: Some(format!("{o}/register")),
                ..Default::default()
            }
        }
        None => bail!("Could not discover OAuth authorization server metadata for {issuer}"),
    };

    validate_endpoint(&as_meta.authorization_endpoint).context("authorization_endpoint")?;
    validate_endpoint(&as_meta.token_endpoint).context("token_endpoint")?;
    if let Some(reg) = &as_meta.registration_endpoint {
        validate_endpoint(reg).context("registration_endpoint")?;
    }
    if !as_meta.code_challenge_methods_supported.is_empty()
        && !as_meta
            .code_challenge_methods_supported
            .iter()
            .any(|m| m == "S256")
    {
        bail!("Authorization server does not support PKCE S256");
    }

    let scopes_supported = if scopes.is_empty() {
        as_meta.scopes_supported.clone()
    } else {
        scopes
    };
    Ok(Flow {
        as_meta,
        resource,
        scopes_supported,
    })
}

async fn prepare_flow(
    client: &reqwest::Client,
    server_url: &str,
    challenge: Option<&str>,
) -> Result<Flow> {
    let challenge = match challenge {
        Some(c) => Some(c.to_string()),
        None => probe_challenge(client, server_url).await,
    };
    discover(client, server_url, challenge.as_deref()).await
}

/// Unauthenticated POST to learn the `resource_metadata` hint from a 401.
async fn probe_challenge(client: &reqwest::Client, server_url: &str) -> Option<String> {
    let resp = client
        .post(server_url)
        .header("content-type", "application/json")
        .header("accept", "application/json, text/event-stream")
        .body(r#"{"jsonrpc":"2.0","id":0,"method":"ping"}"#)
        .send()
        .await
        .ok()?;
    let header = resp.headers().get("www-authenticate")?.to_str().ok()?;
    parse_resource_metadata(header)
}

fn requested_scope(cfg: &McpOAuthConfig, flow: &Flow) -> Option<String> {
    let scopes = if cfg.scopes.is_empty() {
        &flow.scopes_supported
    } else {
        &cfg.scopes
    };
    (!scopes.is_empty()).then(|| scopes.join(" "))
}

pub(crate) async fn register_client(
    client: &reqwest::Client,
    registration_endpoint: &str,
    client_name: &str,
    redirect_uri: &str,
    method: TokenAuthMethod,
) -> Result<Registration> {
    validate_endpoint(registration_endpoint)?;
    let body = serde_json::json!({
        "client_name": client_name,
        "redirect_uris": [redirect_uri],
        "grant_types": ["authorization_code", "refresh_token"],
        "response_types": ["code"],
        "token_endpoint_auth_method": method.as_str(),
    });
    let resp = client
        .post(registration_endpoint)
        .json(&body)
        .send()
        .await
        .context("Dynamic client registration request failed")?;
    let status = resp.status();
    let value: serde_json::Value = resp.json().await.unwrap_or_default();
    if !status.is_success() {
        bail!(
            "Dynamic client registration rejected (HTTP {status}){}",
            oauth_error_summary(&value)
        );
    }
    let id = value
        .get("client_id")
        .and_then(|v| v.as_str())
        .context("Registration response missing client_id")?
        .to_string();
    let secret = value
        .get("client_secret")
        .and_then(|v| v.as_str())
        .map(str::to_string);
    // The server's answer wins; fall back to what we asked for.
    let mut auth_method = value
        .get("token_endpoint_auth_method")
        .and_then(|v| v.as_str())
        .and_then(TokenAuthMethod::parse)
        .unwrap_or(method);
    if auth_method != TokenAuthMethod::None && secret.is_none() {
        bail!("Registration requires {} but no client_secret was issued", auth_method.as_str());
    }
    if auth_method == TokenAuthMethod::None && secret.is_some() {
        auth_method = TokenAuthMethod::ClientSecretPost;
    }
    Ok(Registration {
        client_id: id,
        client_secret: secret,
        auth_method,
    })
}

/// Only the standard `error`/`error_description` fields, never the raw body
/// (which may echo secrets).
fn oauth_error_summary(value: &serde_json::Value) -> String {
    let field = |k: &str| {
        value
            .get(k)
            .and_then(|v| v.as_str())
            .map(|s| s.chars().take(200).collect::<String>())
    };
    match (field("error"), field("error_description")) {
        (Some(e), Some(d)) => format!(": {e}: {d}"),
        (Some(e), None) => format!(": {e}"),
        _ => String::new(),
    }
}

pub(crate) fn build_authorize_url(
    endpoint: &str,
    client_id: &str,
    redirect_uri: &str,
    state: &str,
    challenge: &str,
    scope: Option<&str>,
    resource: Option<&str>,
) -> Result<Url> {
    let mut url = validate_endpoint(endpoint)?;
    {
        let mut q = url.query_pairs_mut();
        q.append_pair("response_type", "code")
            .append_pair("client_id", client_id)
            .append_pair("redirect_uri", redirect_uri)
            .append_pair("state", state)
            .append_pair("code_challenge", challenge)
            .append_pair("code_challenge_method", "S256");
        if let Some(s) = scope {
            q.append_pair("scope", s);
        }
        if let Some(r) = resource {
            q.append_pair("resource", r);
        }
    }
    Ok(url)
}

/// Accept loopback connections until one hits `/callback` with a matching
/// state. Mismatched state or OAuth errors fail the flow.
pub(crate) async fn wait_for_callback(
    listener: tokio::net::TcpListener,
    expected_state: &str,
    expected_issuer: Option<&str>,
) -> Result<String> {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    loop {
        let (mut stream, _) = listener.accept().await?;
        let mut buf = vec![0u8; 8192];
        let n = stream.read(&mut buf).await.unwrap_or(0);
        let req = String::from_utf8_lossy(&buf[..n]);
        let target = req
            .lines()
            .next()
            .and_then(|l| l.split_whitespace().nth(1))
            .unwrap_or("")
            .to_string();
        let parsed = Url::parse(&format!("http://127.0.0.1{target}")).ok();
        let Some(parsed) = parsed.filter(|u| u.path() == CALLBACK_PATH) else {
            let _ = stream
                .write_all(b"HTTP/1.1 404 Not Found\r\ncontent-length: 0\r\nconnection: close\r\n\r\n")
                .await;
            continue;
        };
        let params: HashMap<String, String> = parsed.query_pairs().into_owned().collect();
        let result = if params.get("state").map(String::as_str) != Some(expected_state) {
            Err(anyhow::anyhow!("OAuth callback state mismatch; possible CSRF, aborting"))
        } else if let (Some(iss), Some(exp)) = (params.get("iss"), expected_issuer)
            && iss.trim_end_matches('/') != exp.trim_end_matches('/')
        {
            Err(anyhow::anyhow!("OAuth callback issuer mismatch (RFC 9207), aborting"))
        } else if let Some(err) = params.get("error") {
            let desc: String = params
                .get("error_description")
                .map(|d| d.chars().take(200).collect())
                .unwrap_or_default();
            Err(anyhow::anyhow!("Authorization failed: {err} {desc}"))
        } else if let Some(code) = params.get("code") {
            Ok(code.clone())
        } else {
            Err(anyhow::anyhow!("OAuth callback missing code"))
        };
        let body = if result.is_ok() {
            "<html><body><h2>jcode: authentication complete.</h2>You can close this tab.</body></html>"
        } else {
            "<html><body><h2>jcode: authentication failed.</h2>Return to jcode for details.</body></html>"
        };
        let resp = format!(
            "HTTP/1.1 200 OK\r\ncontent-type: text/html; charset=utf-8\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
            body.len()
        );
        let _ = stream.write_all(resp.as_bytes()).await;
        let _ = stream.shutdown().await;
        return result;
    }
}

#[derive(Debug, Deserialize)]
pub(crate) struct TokenResponse {
    pub access_token: String,
    #[serde(default)]
    pub refresh_token: Option<String>,
    #[serde(default)]
    pub expires_in: Option<u64>,
    #[serde(default)]
    pub scope: Option<String>,
}

async fn token_request(
    client: &reqwest::Client,
    token_endpoint: &str,
    registration: &Registration,
    mut form: Vec<(&str, &str)>,
) -> Result<TokenResponse> {
    validate_endpoint(token_endpoint)?;
    let mut req = client
        .post(token_endpoint)
        .header("accept", "application/json");
    match (registration.auth_method, registration.client_secret.as_deref()) {
        (TokenAuthMethod::ClientSecretBasic, Some(secret)) => {
            let enc = |s: &str| urlencoding::encode(s).into_owned();
            req = req.basic_auth(enc(&registration.client_id), Some(enc(secret)));
        }
        (TokenAuthMethod::ClientSecretPost, Some(secret)) => {
            form.push(("client_id", registration.client_id.as_str()));
            form.push(("client_secret", secret));
        }
        _ => form.push(("client_id", registration.client_id.as_str())),
    }
    let resp = req
        .form(&form)
        .send()
        .await
        .context("OAuth token request failed")?;
    let status = resp.status();
    let text = resp.text().await.unwrap_or_default();
    if !status.is_success() {
        let value = serde_json::from_str(&text).unwrap_or_default();
        bail!(
            "OAuth token endpoint returned HTTP {status}{}",
            oauth_error_summary(&value)
        );
    }
    serde_json::from_str(&text).context("Invalid OAuth token response")
}

pub(crate) async fn exchange_code(
    client: &reqwest::Client,
    token_endpoint: &str,
    registration: &Registration,
    code: &str,
    redirect_uri: &str,
    verifier: &str,
    resource: Option<&str>,
) -> Result<TokenResponse> {
    let mut form = vec![
        ("grant_type", "authorization_code"),
        ("code", code),
        ("redirect_uri", redirect_uri),
        ("code_verifier", verifier),
    ];
    if let Some(r) = resource {
        form.push(("resource", r));
    }
    token_request(client, token_endpoint, registration, form).await
}

pub(crate) async fn refresh(
    client: &reqwest::Client,
    creds: &StoredCredentials,
) -> Result<TokenResponse> {
    let refresh_token = creds.refresh_token.as_deref().context("No refresh token")?;
    let mut form = vec![
        ("grant_type", "refresh_token"),
        ("refresh_token", refresh_token),
    ];
    if let Some(r) = creds.resource.as_deref() {
        form.push(("resource", r));
    }
    let registration = Registration {
        client_id: creds.client_id.clone(),
        client_secret: creds.client_secret.clone(),
        auth_method: creds.token_endpoint_auth_method,
    };
    token_request(client, &creds.token_endpoint, &registration, form).await
}

pub(crate) async fn access_token_at(path: &Path, key: &str) -> Result<Option<String>> {
    let Some(creds) = load_credentials(path, key) else {
        return Ok(None);
    };
    if !creds.is_expired(now_secs()) {
        return Ok(Some(creds.access_token));
    }
    if creds.refresh_token.is_none() {
        // Let the server answer 401 so the user is prompted to re-auth.
        return Ok(None);
    }
    let _guard = REFRESH_LOCK.lock().await;
    // Another task may have refreshed while we waited.
    let Some(mut creds) = load_credentials(path, key) else {
        return Ok(None);
    };
    if !creds.is_expired(now_secs()) {
        return Ok(Some(creds.access_token));
    }
    match refresh(&http_client()?, &creds).await {
        Ok(token) => {
            creds.access_token = token.access_token;
            if token.refresh_token.is_some() {
                creds.refresh_token = token.refresh_token;
            }
            creds.expires_at = token.expires_in.map(|s| now_secs() + s);
            if token.scope.is_some() {
                creds.scope = token.scope;
            }
            let access = creds.access_token.clone();
            save_credentials(path, key, creds)?;
            Ok(Some(access))
        }
        Err(err) => {
            crate::logging::warn(&format!("MCP OAuth refresh failed: {err:#}"));
            Ok(None)
        }
    }
}

// ---------------------------------------------------------------------------
// Credential store
// ---------------------------------------------------------------------------

fn default_store_path() -> Result<PathBuf> {
    Ok(crate::storage::jcode_dir()?.join("mcp-oauth.json"))
}

static STORE_LOCK: StdMutex<()> = StdMutex::new(());

/// Missing file is an empty store; a corrupt file is an error so a write
/// never silently erases other servers' credentials.
fn read_store(path: &Path) -> Result<HashMap<String, StoredCredentials>> {
    match std::fs::read_to_string(path) {
        Ok(s) if s.trim().is_empty() => Ok(HashMap::new()),
        Ok(s) => serde_json::from_str(&s)
            .with_context(|| format!("MCP OAuth store {} is corrupt", path.display())),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(HashMap::new()),
        Err(e) => Err(e).with_context(|| format!("Failed to read {}", path.display())),
    }
}

fn write_store(path: &Path, store: &HashMap<String, StoredCredentials>) -> Result<()> {
    use std::io::Write;
    let dir = path.parent().context("OAuth store path has no parent")?;
    std::fs::create_dir_all(dir)?;
    // NamedTempFile is created 0600 with a random name (no fixed-name race).
    let mut tmp = tempfile::NamedTempFile::new_in(dir)?;
    #[cfg(unix)]
    jcode_core::fs::set_permissions_owner_only(tmp.path())?;
    tmp.write_all(&serde_json::to_vec_pretty(store)?)?;
    tmp.as_file().sync_all()?;
    tmp.persist(path).map_err(|e| e.error)?;
    Ok(())
}

pub(crate) fn load_credentials(path: &Path, key: &str) -> Option<StoredCredentials> {
    let _g = STORE_LOCK.lock().unwrap_or_else(|p| p.into_inner());
    match read_store(path) {
        Ok(mut store) => store.remove(key),
        Err(e) => {
            crate::logging::warn(&format!("{e:#}"));
            None
        }
    }
}

pub(crate) fn save_credentials(path: &Path, key: &str, creds: StoredCredentials) -> Result<()> {
    let _g = STORE_LOCK.lock().unwrap_or_else(|p| p.into_inner());
    let mut store = read_store(path)?;
    store.insert(key.to_string(), creds);
    write_store(path, &store)
}

pub(crate) fn remove_credentials(path: &Path, key: &str) -> Result<bool> {
    let _g = STORE_LOCK.lock().unwrap_or_else(|p| p.into_inner());
    let mut store = read_store(path)?;
    let removed = store.remove(key).is_some();
    if removed {
        write_store(path, &store)?;
    }
    Ok(removed)
}

#[cfg(test)]
#[path = "oauth_tests.rs"]
mod tests;
