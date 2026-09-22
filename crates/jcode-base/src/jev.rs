//! Shared Jev connection resolution. Resolution never falls back to another provider.

use anyhow::{Context, Result, bail, ensure};
use std::time::Duration;

pub use crate::config::JevConfig;
pub const MAX_TIMEOUT_MS: u64 = 30_000;
pub const LOCAL_TIMEOUT_MS: u64 = 15_000;

#[derive(Clone)]
pub struct ResolvedJev {
    pub endpoint: String,
    pub model: String,
    pub api_key: Option<String>,
    pub timeout: Duration,
}

pub fn resolve(config: &JevConfig) -> Result<ResolvedJev> {
    resolve_with_timeout(config, Duration::from_secs(5))
}

/// Preserve a consumer's historical hosted deadline unless explicitly configured.
pub fn resolve_with_timeout(config: &JevConfig, hosted_default: Duration) -> Result<ResolvedJev> {
    resolve_with_loader(config, hosted_default, |name, file| match file {
        Some(file) => crate::provider_catalog::load_api_key_from_env_or_config(name, file),
        None => std::env::var(name).ok(),
    })
}

fn nonempty(value: Option<&str>) -> Option<&str> {
    value.map(str::trim).filter(|value| !value.is_empty())
}

fn resolve_with_loader(
    config: &JevConfig,
    hosted_default: Duration,
    mut key_loader: impl FnMut(&str, Option<&str>) -> Option<String>,
) -> Result<ResolvedJev> {
    let provider = config.provider.trim().to_ascii_lowercase();
    let (default_endpoint, default_model, default_key, key_file) = match provider.as_str() {
        "typesafe" => (
            "https://api.typesafe.ai/v1/systemone",
            "jev-latest",
            Some("TYPESAFE_API_KEY"),
            Some("typesafe.env"),
        ),
        "openjev" => (
            "http://127.0.0.1:8791/v1/systemone",
            "jev-latest",
            None,
            None,
        ),
        "openrouter" => (
            "https://openrouter.ai/api/alpha/decisions",
            "typesafe/jev-1.13",
            Some("OPENROUTER_API_KEY"),
            Some("openrouter.env"),
        ),
        _ => bail!("unknown Jev provider: {}", config.provider),
    };
    let endpoint = match nonempty(config.base_url.as_deref()) {
        None => default_endpoint.to_string(),
        Some(base) => {
            let base = base.trim_end_matches('/');
            let mut url = url::Url::parse(base).context("invalid Jev base URL")?;
            ensure!(
                matches!(url.scheme(), "http" | "https") && url.host_str().is_some(),
                "Jev URL must use http or https"
            );
            ensure!(
                url.username().is_empty()
                    && url.password().is_none()
                    && url.query().is_none()
                    && url.fragment().is_none(),
                "Jev URL must not contain credentials, query, or fragment"
            );
            let path = url.path().trim_end_matches('/');
            let path = if provider == "openrouter" {
                if path.ends_with("/decisions") {
                    path.to_string()
                } else if path.is_empty() {
                    "/api/alpha/decisions".into()
                } else {
                    format!("{path}/decisions")
                }
            } else if path.ends_with("/systemone") {
                path.to_string()
            } else if path.is_empty() {
                "/v1/systemone".into()
            } else {
                format!("{path}/systemone")
            };
            url.set_path(&path);
            url.to_string()
        }
    };
    let key_env = nonempty(config.api_key_env.as_deref()).or(default_key);
    let api_key = key_env
        .and_then(|name| key_loader(name, key_file))
        .and_then(|key| nonempty(Some(&key)).map(str::to_string));
    ensure!(
        (provider == "openjev" && nonempty(config.api_key_env.as_deref()).is_none())
            || api_key.is_some(),
        "Jev provider {provider} requires an API key"
    );
    let default_timeout = if provider == "openjev" {
        LOCAL_TIMEOUT_MS
    } else {
        hosted_default.as_millis().min(MAX_TIMEOUT_MS as u128) as u64
    };
    Ok(ResolvedJev {
        endpoint,
        model: nonempty(config.model.as_deref())
            .unwrap_or(default_model)
            .to_string(),
        api_key,
        timeout: Duration::from_millis(
            config
                .timeout_ms
                .unwrap_or(default_timeout)
                .clamp(1, MAX_TIMEOUT_MS),
        ),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    fn local() -> JevConfig {
        JevConfig {
            provider: "openjev".into(),
            ..Default::default()
        }
    }
    #[test]
    fn local_never_loads_hosted_credentials() {
        let resolved = resolve_with_loader(&local(), Duration::from_secs(1), |_, _| {
            panic!("must not load a hosted key")
        })
        .unwrap();
        assert_eq!(resolved.endpoint, "http://127.0.0.1:8791/v1/systemone");
        assert_eq!(resolved.model, "jev-latest");
        assert!(resolved.api_key.is_none());
        assert_eq!(resolved.timeout, Duration::from_secs(15));
    }
    #[test]
    fn hosted_requires_auth_and_uses_saved_key_loader() {
        assert!(
            resolve_with_loader(&JevConfig::default(), Duration::from_secs(1), |_, _| None)
                .is_err()
        );
        let resolved = resolve_with_loader(
            &JevConfig::default(),
            Duration::from_millis(1500),
            |key, file| {
                assert_eq!(key, "TYPESAFE_API_KEY");
                assert_eq!(file, Some("typesafe.env"));
                Some(" secret ".into())
            },
        )
        .unwrap();
        assert_eq!(resolved.api_key.as_deref(), Some("secret"));
        assert_eq!(resolved.endpoint, "https://api.typesafe.ai/v1/systemone");
        assert_eq!(resolved.timeout, Duration::from_millis(1500));
    }
    #[test]
    fn unknown_provider_never_loads_key_or_falls_back() {
        let cfg = JevConfig {
            provider: "typo".into(),
            ..Default::default()
        };
        assert!(
            resolve_with_loader(&cfg, Duration::from_secs(1), |_, _| panic!(
                "no auth lookup"
            ))
            .is_err()
        );
    }
    #[test]
    fn local_explicit_auth_uses_only_named_environment_variable() {
        let cfg = JevConfig {
            api_key_env: Some("LOCAL_TOKEN".into()),
            ..local()
        };
        let resolved = resolve_with_loader(&cfg, Duration::from_secs(1), |key, file| {
            assert_eq!(key, "LOCAL_TOKEN");
            assert_eq!(file, None);
            Some("local-key".into())
        })
        .unwrap();
        assert_eq!(resolved.api_key.as_deref(), Some("local-key"));
    }
    #[test]
    fn explicit_local_key_must_exist_and_be_nonempty() {
        let cfg = JevConfig {
            api_key_env: Some("LOCAL_KEY".into()),
            ..local()
        };
        for missing in [None, Some("   ".to_string())] {
            assert!(
                resolve_with_loader(&cfg, Duration::from_secs(1), |_, _| missing.clone()).is_err()
            );
        }
    }

    #[test]
    fn overrides_normalize_endpoint_model_and_bound_timeout() {
        for base in [
            "http://localhost:8791",
            "http://localhost:8791/v1/",
            "http://localhost:8791/v1/systemone/",
        ] {
            for (millis, expected) in [(0, 1), (42, 42), (u64::MAX, MAX_TIMEOUT_MS)] {
                let cfg = JevConfig {
                    base_url: Some(base.into()),
                    model: Some(" custom ".into()),
                    timeout_ms: Some(millis),
                    ..local()
                };
                let resolved = resolve(&cfg).unwrap();
                assert_eq!(resolved.endpoint, "http://localhost:8791/v1/systemone");
                assert_eq!(resolved.model, "custom");
                assert_eq!(resolved.timeout, Duration::from_millis(expected));
            }
        }
        for base in [
            "file:///secret",
            "http://user:secret@localhost",
            "http://localhost?secret=x",
        ] {
            assert!(
                resolve(&JevConfig {
                    base_url: Some(base.into()),
                    ..local()
                })
                .is_err()
            );
        }
    }
    #[test]
    fn openrouter_legacy_defaults() {
        let cfg = JevConfig {
            provider: "openrouter".into(),
            ..Default::default()
        };
        let resolved = resolve_with_loader(&cfg, Duration::from_secs(5), |key, file| {
            assert_eq!(key, "OPENROUTER_API_KEY");
            assert_eq!(file, Some("openrouter.env"));
            Some("key".into())
        })
        .unwrap();
        assert_eq!(
            resolved.endpoint,
            "https://openrouter.ai/api/alpha/decisions"
        );
        assert_eq!(resolved.model, "typesafe/jev-1.13");
    }
}
