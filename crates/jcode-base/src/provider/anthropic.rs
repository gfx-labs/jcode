//! Anthropic provider shared helpers (compatibility shim).
//!
//! The direct Anthropic Messages API *runtime* (`AnthropicProvider`) now lives
//! in the downstream `jcode-provider-anthropic-runtime` crate so provider
//! edits do not rebuild the base -> app-core -> tui spine. The binary's
//! composition root registers it via [`crate::provider::external`].
//!
//! Base keeps the pieces its own auth/usage/sidecar code (and the runtime
//! crate) share:
//! - the OAuth attribution headers + Claude CLI user agent used for
//!   subscription API calls,
//! - API-key resolution (`load_anthropic_api_key`, `has_anthropic_api_key`),
//! - the process-wide cache-TTL toggle, and
//! - the static model list.

use anyhow::{Context, Result};
use std::sync::atomic::{AtomicU8, Ordering};
use uuid::Uuid;

pub use jcode_provider_core::CredentialMode as AnthropicCredentialMode;
use jcode_provider_core::{
    ANTHROPIC_OAUTH_BETA_HEADERS, anthropic_effectively_1m,
    anthropic_stainless_arch as stainless_arch, anthropic_stainless_os as stainless_os,
};

// 0 follows persisted configuration, 1/2 are explicit process-local overrides.
static CACHE_TTL_1H: AtomicU8 = AtomicU8::new(0);

/// Override cache TTL for this process. UI preferences should use Config instead.
pub fn set_cache_ttl_1h(enabled: bool) {
    CACHE_TTL_1H.store(if enabled { 2 } else { 1 }, Ordering::Relaxed);
}

/// Check if 1-hour cache TTL is enabled
pub fn is_cache_ttl_1h() -> bool {
    match CACHE_TTL_1H.load(Ordering::Relaxed) {
        1 => false,
        2 => true,
        _ => crate::config::config().provider.anthropic_cache_ttl_1h,
    }
}

/// Claude Code CLI version we present on OAuth requests.
///
/// Anthropic gates newer models on this: a version below a model's floor is
/// rejected and the request silently falls back to an older model (for
/// example claude-opus-5-5 requires 2.1.280, and 2.1.257 fell back to
/// opus-5). Keep this at or above the newest model we intend to reach, and
/// update it together with `AVAILABLE_MODELS`.
///
/// This is the single source of truth: the OAuth preflight `appVersion` is
/// derived from it, so the User-Agent and the preflight body can never drift.
pub const CLAUDE_CLI_VERSION: &str = "2.1.280";

/// User-Agent for OAuth requests, matching the official Claude Code CLI.
///
/// Kept in sync with [`CLAUDE_CLI_VERSION`] by a unit test rather than by
/// const concatenation, which would need an extra dependency.
pub const CLAUDE_CLI_USER_AGENT: &str = "claude-cli/2.1.280 (external, sdk-cli)";

pub const OAUTH_BETA_HEADERS: &str = ANTHROPIC_OAUTH_BETA_HEADERS;

/// Whether a model id effectively runs with the 1M-token context beta.
pub fn effectively_1m(model: &str) -> bool {
    anthropic_effectively_1m(model)
}

pub fn new_oauth_request_id() -> String {
    Uuid::new_v4().to_string()
}

/// Attach the OAuth attribution headers the official Claude CLI sends.
/// Shared by the runtime crate's request path and base's usage probes.
pub fn apply_oauth_attribution_headers(
    req: reqwest::RequestBuilder,
    session_id: &str,
) -> reqwest::RequestBuilder {
    req.header("x-client-request-id", new_oauth_request_id())
        .header("x-app", "cli")
        .header("X-Claude-Code-Session-Id", session_id)
        .header("X-Stainless-Arch", stainless_arch())
        .header("X-Stainless-Lang", "js")
        .header("X-Stainless-OS", stainless_os())
        .header("X-Stainless-Package-Version", "0.81.0")
        .header("X-Stainless-Retry-Count", "0")
        .header("X-Stainless-Runtime", "node")
        .header("X-Stainless-Runtime-Version", "v24.3.0")
        .header("X-Stainless-Timeout", "600")
        .header("anthropic-dangerous-direct-browser-access", "true")
}

/// Available models
pub const AVAILABLE_MODELS: &[&str] = &[
    // Requires CLAUDE_CLI_VERSION >= 2.1.280; older clients are rejected and
    // silently fall back to claude-opus-5.
    "claude-opus-5-5",
    "claude-opus-5",
    "claude-fable-5-1",
    "claude-fable-5",
    "claude-opus-4-8",
    "claude-opus-4-6",
    "claude-opus-4-6[1m]",
    "claude-sonnet-5",
    "claude-sonnet-4-6",
    "claude-sonnet-4-6[1m]",
    "claude-haiku-4-5",
    "claude-opus-4-5",
    "claude-sonnet-4-5",
    "claude-sonnet-4-20250514",
];

pub fn load_anthropic_api_key() -> Result<String> {
    if std::env::var("JCODE_ANTHROPIC_AUTH")
        .ok()
        .is_some_and(|value| value.eq_ignore_ascii_case("none"))
    {
        return Ok(String::new());
    }
    if let Ok(env_name) = std::env::var("JCODE_ANTHROPIC_API_KEY_NAME") {
        let env_name = env_name.trim();
        if !env_name.is_empty() {
            if let Ok(value) = std::env::var(env_name)
                && !value.trim().is_empty()
            {
                return Ok(value);
            }
            if let Ok(env_file) = std::env::var("JCODE_ANTHROPIC_ENV_FILE")
                && let Some(value) = crate::provider_catalog::load_env_value_from_config_file(
                    env_name,
                    env_file.trim(),
                )
                && !value.trim().is_empty()
            {
                return Ok(value);
            }
            anyhow::bail!(
                "Anthropic-compatible profile credential '{}' is not configured",
                env_name
            );
        }
    }
    if let Ok(value) = std::env::var("ANTHROPIC_AUTH_TOKEN")
        && !value.trim().is_empty()
    {
        return Ok(value);
    }
    let key = crate::provider_catalog::load_api_key_from_env_or_config(
        "ANTHROPIC_API_KEY",
        "anthropic.env",
    )
    .context("No Anthropic API key found")?;
    if std::env::var("JCODE_LOG_SERVICE_TIER").is_ok() {
        let prefix: String = key.chars().take(14).collect();
        eprintln!(
            "[anthropic] resolved API key prefix={prefix}... (len={})",
            key.len()
        );
    }
    Ok(key)
}

pub fn has_anthropic_api_key() -> bool {
    load_anthropic_api_key().is_ok()
}

#[cfg(test)]
mod version_gate_tests {
    use super::*;

    /// The User-Agent is written out literally, so it can drift from
    /// [`CLAUDE_CLI_VERSION`]. Anthropic gates models on the advertised
    /// version, and a stale one is rejected and silently downgrades the model,
    /// so pin the two together.
    #[test]
    fn user_agent_advertises_the_declared_cli_version() {
        assert_eq!(
            CLAUDE_CLI_USER_AGENT,
            format!("claude-cli/{CLAUDE_CLI_VERSION} (external, sdk-cli)"),
            "CLAUDE_CLI_USER_AGENT must embed CLAUDE_CLI_VERSION"
        );
    }

    /// claude-opus-5-5 is rejected below 2.1.280 and falls back to opus-5.
    /// Offering the model while advertising an older client would reintroduce
    /// exactly that silent downgrade.
    #[test]
    fn advertised_version_meets_the_floor_of_every_offered_model() {
        fn parse(version: &str) -> (u32, u32, u32) {
            let mut parts = version.split('.').map(|p| p.parse::<u32>().unwrap_or(0));
            (
                parts.next().unwrap_or(0),
                parts.next().unwrap_or(0),
                parts.next().unwrap_or(0),
            )
        }
        // (model id, minimum claude-cli version that may request it)
        const MODEL_VERSION_FLOORS: &[(&str, &str)] = &[("claude-opus-5-5", "2.1.280")];

        let advertised = parse(CLAUDE_CLI_VERSION);
        for (model, floor) in MODEL_VERSION_FLOORS {
            if AVAILABLE_MODELS.contains(model) {
                assert!(
                    advertised >= parse(floor),
                    "{model} requires claude-cli >= {floor}, but we advertise {CLAUDE_CLI_VERSION}; \
                     the request would be rejected and silently fall back to an older model"
                );
            }
        }
    }
}
