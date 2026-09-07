//! One reporting destination for usage events and separately consented transcripts.

const DEFAULT_BASE_URL: &str = "https://telemetry.jcode.sh";
const BASE_URL_ENV: &str = "JCODE_TELEMETRY_BASE_URL";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TelemetryEndpoints {
    pub event: reqwest::Url,
    pub transcript: reqwest::Url,
}

/// Resolve reporting destinations without sending traffic or creating an identity.
///
/// An invalid explicit override fails closed, never reverting to the public
/// service. Both streams use the same base so content cannot accidentally keep
/// going to the public service after usage reporting is redirected locally.
pub fn reporting_endpoints() -> Result<TelemetryEndpoints, &'static str> {
    match std::env::var(BASE_URL_ENV) {
        Ok(value) => parse_base_url(&value),
        Err(std::env::VarError::NotPresent) => parse_base_url(DEFAULT_BASE_URL),
        Err(std::env::VarError::NotUnicode(_)) => {
            Err("JCODE_TELEMETRY_BASE_URL must be valid UTF-8")
        }
    }
}

fn parse_base_url(value: &str) -> Result<TelemetryEndpoints, &'static str> {
    let mut base = reqwest::Url::parse(value.trim())
        .map_err(|_| "JCODE_TELEMETRY_BASE_URL must be an absolute HTTP(S) URL")?;
    if !matches!(base.scheme(), "http" | "https") || base.host_str().is_none() {
        return Err("JCODE_TELEMETRY_BASE_URL must be an absolute HTTP(S) URL");
    }
    if !base.username().is_empty()
        || base.password().is_some()
        || base.query().is_some()
        || base.fragment().is_some()
    {
        return Err(
            "JCODE_TELEMETRY_BASE_URL must not contain credentials, a query, or a fragment",
        );
    }
    let prefix = base.path().trim_end_matches('/').to_string();
    base.set_path(&format!("{prefix}/v1/event"));
    let event = base.clone();
    base.set_path(&format!("{prefix}/v1/transcript"));
    Ok(TelemetryEndpoints {
        event,
        transcript: base,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_endpoints_preserve_production_paths() {
        let endpoints = parse_base_url(DEFAULT_BASE_URL).unwrap();
        assert_eq!(
            endpoints.event.as_str(),
            "https://telemetry.jcode.sh/v1/event"
        );
        assert_eq!(
            endpoints.transcript.as_str(),
            "https://telemetry.jcode.sh/v1/transcript"
        );
    }

    #[test]
    fn local_bases_support_ports_ipv6_prefixes_and_trailing_slashes() {
        for (base, expected) in [
            ("http://127.0.0.1:4318", "http://127.0.0.1:4318"),
            ("http://localhost:8080/", "http://localhost:8080"),
            ("http://[::1]:8080", "http://[::1]:8080"),
            (
                "https://collector.test/jcode///",
                "https://collector.test/jcode",
            ),
            (" http://localhost:8080/ ", "http://localhost:8080"),
        ] {
            let endpoints = parse_base_url(base).unwrap();
            assert_eq!(endpoints.event.as_str(), format!("{expected}/v1/event"));
            assert_eq!(
                endpoints.transcript.as_str(),
                format!("{expected}/v1/transcript")
            );
        }
    }

    #[test]
    fn malformed_or_credential_bearing_bases_fail_closed() {
        for base in [
            "",
            " ",
            "localhost:8080",
            "/relative",
            "file:///tmp/events",
            "ftp://collector.test",
            "http://user:secret@localhost:8080",
            "http://user@localhost:8080",
            "http://localhost:8080?token=secret",
            "http://localhost:8080#secret",
        ] {
            let error = parse_base_url(base).expect_err(base);
            assert!(!error.contains("secret"));
        }
    }
}
