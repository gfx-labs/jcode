use serde::{Deserialize, Serialize};

/// Deliberately excludes account labels, credentials, activity, and raw API errors.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct MobileProviderUsage {
    pub provider: String,
    pub display_name: String,
    /// True when at least one authenticated remote quota is available.
    pub available: bool,
    /// Friendly unavailability or partial-account failure description.
    pub error: Option<String>,
    pub limits: Vec<MobileUsageLimit>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct MobileUsageLimit {
    pub name: String,
    /// Provider-reported quota utilization, never inferred from local token usage.
    pub used_percent: f32,
    /// Complement of quota utilization, floored at zero for overage.
    pub remaining_percent: f32,
    /// Provider reset timestamp, or null when unsupported.
    pub resets_at: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ServerEvent, encode_event};

    #[test]
    fn mobile_usage_wire_contract_preserves_nulls_and_percentages() {
        let event = ServerEvent::MobileUsage {
            id: 42,
            scope: "configured".into(),
            providers: vec![MobileProviderUsage {
                provider: "openai".into(),
                display_name: "OpenAI".into(),
                available: true,
                error: None,
                limits: vec![MobileUsageLimit {
                    name: "5-hour window".into(),
                    used_percent: 25.0,
                    remaining_percent: 75.0,
                    resets_at: None,
                }],
            }],
            error: None,
            fetched_at_unix_secs: 123,
            from_cache: true,
        };
        let value: serde_json::Value = serde_json::from_str(&encode_event(&event)).unwrap();
        assert_eq!(value["type"], "mobile_usage");
        assert_eq!(value["id"], 42);
        assert!(value["error"].is_null());
        assert_eq!(
            value["providers"][0]["limits"][0]["remaining_percent"],
            75.0
        );
        assert!(
            value["providers"][0]["limits"][0]
                .get("resets_at")
                .unwrap()
                .is_null()
        );
        let _: ServerEvent = serde_json::from_value(value).unwrap();
    }
}
