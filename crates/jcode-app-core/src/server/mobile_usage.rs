//! Read-only projection of the existing authenticated `/usage` facilities.
//! No session/agent handles enter this module. Only this small cache is locked
//! across a fetch, coalescing concurrent mobile polls without blocking agents.
use crate::protocol::{MobileProviderUsage, MobileUsageLimit, ServerEvent};
use crate::usage::ProviderUsage;
use std::collections::BTreeMap;
use std::sync::OnceLock;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use tokio::sync::Mutex;

const CACHE_TTL: Duration = Duration::from_secs(120);
const FETCH_TIMEOUT: Duration = Duration::from_secs(15);
const UNAVAILABLE: &str = "Provider quota information is unavailable.";

#[derive(Clone)]
struct Snapshot {
    providers: Vec<MobileProviderUsage>,
    error: Option<String>,
    fetched_at_unix_secs: u64,
}

#[derive(Default)]
struct UsageCache {
    snapshot: Option<(Instant, Snapshot)>,
}

impl UsageCache {
    async fn get<F, Fut>(&mut self, timeout: Duration, fetch: F) -> (Snapshot, bool)
    where
        F: FnOnce() -> Fut,
        Fut: std::future::Future<Output = Vec<ProviderUsage>>,
    {
        if let Some((at, snapshot)) = &self.snapshot
            && at.elapsed() < CACHE_TTL
        {
            return (snapshot.clone(), true);
        }
        // The underlying facility already caches provider reports and applies
        // authentication/rate-limit backoff. This wrapper also caches empty and
        // failed requests, and prevents parallel mobile polls from stampeding it.
        let (providers, error) = match tokio::time::timeout(timeout, fetch()).await {
            Ok(reports) => {
                let providers = sanitize_reports(reports);
                let error = providers
                    .is_empty()
                    .then(|| "No provider quota reports are available on this server.".to_owned());
                (providers, error)
            }
            Err(_) => (
                Vec::new(),
                Some("Provider usage request timed out. Try again later.".into()),
            ),
        };
        let snapshot = Snapshot {
            providers,
            error,
            fetched_at_unix_secs: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs(),
        };
        self.snapshot = Some((Instant::now(), snapshot.clone()));
        (snapshot, false)
    }
}

pub(super) async fn usage_event(id: u64) -> ServerEvent {
    static CACHE: OnceLock<Mutex<UsageCache>> = OnceLock::new();
    let (snapshot, from_cache) = CACHE
        .get_or_init(|| Mutex::new(UsageCache::default()))
        .lock()
        .await
        .get(FETCH_TIMEOUT, crate::usage::fetch_all_provider_usage)
        .await;
    ServerEvent::MobileUsage {
        id,
        scope: "configured".into(),
        providers: snapshot.providers,
        error: snapshot.error,
        fetched_at_unix_secs: snapshot.fetched_at_unix_secs,
        from_cache,
    }
}

/// Only allow known public provider names. `/usage` display strings can include
/// arbitrary account labels/emails, so never echo them or attempt generic splitting.
fn provider_identity(name: &str) -> (String, String) {
    for (prefix, id, display) in [
        ("Anthropic", "anthropic", "Anthropic"),
        ("OpenAI", "openai", "OpenAI"),
        ("OpenRouter", "openrouter", "OpenRouter"),
        ("GitHub Copilot", "copilot", "GitHub Copilot"),
        ("Antigravity", "antigravity", "Antigravity"),
        ("Google Gemini", "gemini", "Google Gemini"),
        ("Cursor", "cursor", "Cursor"),
    ] {
        if name == prefix || name.starts_with(&format!("{prefix} ")) {
            return (id.into(), display.into());
        }
    }
    for profile in crate::provider_catalog::openai_compatible_profiles() {
        if name == profile.display_name || name == format!("{} (API key)", profile.display_name) {
            return (profile.id.into(), profile.display_name.into());
        }
    }
    ("other".into(), "Other provider".into())
}

fn sanitize_reports(reports: Vec<ProviderUsage>) -> Vec<MobileProviderUsage> {
    let mut grouped: BTreeMap<String, (String, Vec<ProviderUsage>)> = BTreeMap::new();
    for report in reports {
        let (id, display) = provider_identity(&report.provider_name);
        grouped
            .entry(id)
            .or_insert_with(|| (display, Vec::new()))
            .1
            .push(report);
    }
    grouped
        .into_iter()
        .map(|(provider, (display_name, reports))| {
            let multiple = reports.len() > 1;
            let mut limits = Vec::new();
            let mut unavailable = false;
            for (index, report) in reports.into_iter().enumerate() {
                // Errors may embed credentials, account ids, or response bodies.
                // Never pass them through, even when accompanied by stale limits.
                if report.error.is_some() {
                    unavailable = true;
                    continue;
                }
                let before = limits.len();
                for limit in report.limits {
                    if !limit.usage_percent.is_finite() || limit.usage_percent < 0.0 {
                        unavailable = true;
                        continue;
                    }
                    // Provider names/labels/ids are free-form, even inside a
                    // quota window. Only exact public labels may reach mobile.
                    let name = match limit.name.as_str() {
                        "5-hour window" => "5-hour window".into(),
                        "7-day window" => "7-day window".into(),
                        "7-day Opus window" => "7-day Opus window".into(),
                        "7-day Sonnet window" => "7-day Sonnet window".into(),
                        "Credits" => "Credits".into(),
                        "Key limit" => "Key limit".into(),
                        _ => format!("Limit {}", limits.len() - before + 1),
                    };
                    let name = if multiple {
                        format!("Source {}: {name}", index + 1)
                    } else {
                        name
                    };
                    // Only forward timestamps, never unexpected free-form server strings.
                    let resets_at = limit.resets_at.filter(|s| {
                        chrono::DateTime::parse_from_rfc3339(s).is_ok()
                            || chrono::NaiveDate::parse_from_str(s, "%Y-%m-%d").is_ok()
                    });
                    limits.push(MobileUsageLimit {
                        name,
                        used_percent: limit.usage_percent,
                        remaining_percent: (100.0 - limit.usage_percent).max(0.0),
                        resets_at,
                    });
                }
                unavailable |= limits.len() == before;
            }
            let available = !limits.is_empty();
            MobileProviderUsage {
                provider,
                display_name,
                available,
                error: if !available {
                    Some(UNAVAILABLE.into())
                } else if unavailable {
                    Some("Quota information is unavailable for some configured sources.".into())
                } else {
                    None
                },
                limits,
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::usage::UsageLimit;

    fn report(name: &str, used: f32) -> ProviderUsage {
        ProviderUsage {
            provider_name: name.into(),
            limits: vec![UsageLimit {
                name: "5-hour window".into(),
                usage_percent: used,
                resets_at: Some("2026-09-11T05:00:00Z".into()),
            }],
            ..Default::default()
        }
    }

    #[test]
    fn private_fields_and_failed_quotas_never_reach_phone() {
        let mut failed = report("OpenAI - secret@example.com", 0.0);
        failed.error = Some("Bearer credential-secret, account private-id".into());
        failed
            .extra_info
            .push(("Account".into(), "secret@example.com".into()));
        let rows = sanitize_reports(vec![failed]);
        assert!(!rows[0].available);
        assert!(rows[0].limits.is_empty());
        let wire = serde_json::to_string(&rows).unwrap();
        for private in ["secret", "Bearer", "private-id", "Account"] {
            assert!(!wire.contains(private));
        }
    }

    #[test]
    fn arbitrary_quota_names_are_replaced_not_trimmed_or_forwarded() {
        let mut usage = report("OpenAI", 25.0);
        usage.limits = [
            "secret@example.com",
            "5-hour window (account-private-id)",
            "7-day private-organization window",
            "5-hour window",
        ]
        .into_iter()
        .map(|name| UsageLimit {
            name: name.into(),
            usage_percent: 25.0,
            resets_at: None,
        })
        .collect();
        let rows = sanitize_reports(vec![usage]);
        let names: Vec<_> = rows[0]
            .limits
            .iter()
            .map(|limit| limit.name.as_str())
            .collect();
        assert_eq!(names, ["Limit 1", "Limit 2", "Limit 3", "5-hour window"]);
        let wire = serde_json::to_string(&rows).unwrap();
        for private in ["secret", "private", "organization", "example.com"] {
            assert!(!wire.contains(private));
        }
        assert!(rows[0].available);
        assert_eq!(rows[0].limits[0].remaining_percent, 75.0);
    }

    #[test]
    fn independent_accounts_are_not_summed_and_overage_has_no_negative_remaining() {
        let rows = sanitize_reports(vec![
            report("OpenAI - private-a", 20.0),
            report("OpenAI - private-b", 110.0),
        ]);
        assert_eq!(rows.len(), 1);
        assert!(rows[0].available);
        assert_eq!(rows[0].limits[0].remaining_percent, 80.0);
        assert_eq!(rows[0].limits[1].remaining_percent, 0.0);
        assert_eq!(rows[0].limits[1].used_percent, 110.0);
        assert_eq!(rows[0].limits[0].name, "Source 1: 5-hour window");
        assert!(!serde_json::to_string(&rows).unwrap().contains("private"));
    }

    #[test]
    fn missing_invalid_and_partial_usage_is_explicit() {
        let rows = sanitize_reports(vec![
            report("OpenAI", f32::NAN),
            report("OpenAI API key", 50.0),
            ProviderUsage {
                provider_name: "private custom account".into(),
                ..Default::default()
            },
        ]);
        assert!(rows[0].available);
        assert!(rows[0].error.is_some());
        assert_eq!(rows[0].limits.len(), 1);
        assert!(!rows[1].available);
        assert_eq!(rows[1].display_name, "Other provider");
        assert!(rows[1].limits.is_empty());
    }

    #[tokio::test]
    async fn empty_and_successful_results_are_cached_without_refetching() {
        let mut cache = UsageCache::default();
        let (first, cached) = cache.get(FETCH_TIMEOUT, || async { Vec::new() }).await;
        assert!(!cached);
        assert!(first.error.is_some());
        let (_, cached) = cache
            .get(FETCH_TIMEOUT, || async {
                panic!("must not refetch empty result")
            })
            .await;
        assert!(cached);
        cache.snapshot.as_mut().unwrap().0 = Instant::now() - CACHE_TTL;
        let (fresh, cached) = cache
            .get(FETCH_TIMEOUT, || async { vec![report("OpenAI", 10.0)] })
            .await;
        assert!(!cached);
        assert!(fresh.error.is_none());
        let (again, cached) = cache
            .get(FETCH_TIMEOUT, || async {
                panic!("must not refetch fresh result")
            })
            .await;
        assert!(cached);
        assert_eq!(again.providers, fresh.providers);
        assert_eq!(again.fetched_at_unix_secs, fresh.fetched_at_unix_secs);
    }

    #[tokio::test]
    async fn timeout_is_friendly_and_cached_instead_of_retried() {
        let mut cache = UsageCache::default();
        let (first, cached) = cache
            .get(Duration::from_millis(1), || async {
                std::future::pending::<Vec<ProviderUsage>>().await
            })
            .await;
        assert!(!cached);
        assert!(first.providers.is_empty());
        assert!(first.error.as_deref().unwrap().contains("timed out"));
        let (_, cached) = cache
            .get(FETCH_TIMEOUT, || async {
                panic!("must not immediately retry a timed-out provider fetch")
            })
            .await;
        assert!(cached);
    }
}
