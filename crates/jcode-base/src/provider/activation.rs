use anyhow::Result;
use jcode_provider_core::{ActiveProvider, provider_key};

/// Stable product/runtime identity selected by login or provider initialization.
///
/// This intentionally differs from the lower-level [`ActiveProvider`] execution slot.
/// For example Azure OpenAI currently reuses the OpenAI-compatible/OpenRouter HTTP
/// transport, but its runtime identity is still Azure OpenAI.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RuntimeProviderId {
    Jcode,
    Claude,
    ClaudeApiKey,
    OpenAi,
    OpenAiApiKey,
    OpenRouter,
    OpenAiCompatible,
    AzureOpenAi,
    Bedrock,
    Cursor,
    GrokBuild,
    Copilot,
    Gemini,
    Antigravity,
    AutoImport,
}

impl RuntimeProviderId {
    pub const fn key(self) -> &'static str {
        match self {
            Self::Jcode => "jcode",
            Self::Claude => "claude",
            Self::ClaudeApiKey => "claude-api",
            Self::OpenAi => "openai",
            Self::OpenAiApiKey => "openai-api",
            Self::OpenRouter => "openrouter",
            Self::OpenAiCompatible => "openai-compatible",
            Self::AzureOpenAi => "azure-openai",
            Self::Bedrock => "bedrock",
            Self::Cursor => "cursor",
            Self::GrokBuild => "grok-build",
            Self::Copilot => "copilot",
            Self::Gemini => "gemini",
            Self::Antigravity => "antigravity",
            Self::AutoImport => "auto-import",
        }
    }

    pub const fn label(self) -> &'static str {
        match self {
            Self::Jcode => "Jcode Subscription",
            Self::Claude => "Anthropic/Claude",
            Self::ClaudeApiKey => "Anthropic API",
            Self::OpenAi => "OpenAI",
            Self::OpenAiApiKey => "OpenAI API",
            Self::OpenRouter => "OpenRouter",
            Self::OpenAiCompatible => "OpenAI-compatible",
            Self::AzureOpenAi => "Azure OpenAI",
            Self::Bedrock => "AWS Bedrock",
            Self::Cursor => "Cursor",
            Self::GrokBuild => "Grok Build",
            Self::Copilot => "GitHub Copilot",
            Self::Gemini => "Gemini",
            Self::Antigravity => "Antigravity",
            Self::AutoImport => "Auto Import",
        }
    }
}

/// How model routing should be represented in the current process.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RuntimeSelection {
    /// Select the initial execution slot without restricting later model switches.
    Initial(ActiveProvider),
    /// Do not select a specific route. Optionally set an active provider hint for UI/session context.
    Unlocked { active_hint: Option<ActiveProvider> },
    /// Leave existing routing env untouched.
    Unchanged,
}

impl RuntimeSelection {
    fn log_value(self) -> &'static str {
        match self {
            Self::Initial(_) => "initial",
            Self::Unlocked { .. } => "unlocked",
            Self::Unchanged => "unchanged",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RuntimeModelHint {
    pub env_key: &'static str,
    pub model: String,
}

impl RuntimeModelHint {
    pub fn new(env_key: &'static str, model: impl Into<String>) -> Self {
        Self {
            env_key,
            model: model.into(),
        }
    }
}

/// Typed activation plan shared by CLI, TUI, and bootstrap code.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProviderActivation {
    pub runtime_id: RuntimeProviderId,
    pub selection: RuntimeSelection,
    pub model_hint: Option<RuntimeModelHint>,
}

impl ProviderActivation {
    pub fn new(runtime_id: RuntimeProviderId, selection: RuntimeSelection) -> Self {
        Self {
            runtime_id,
            selection,
            model_hint: None,
        }
    }

    pub fn with_model_hint(mut self, env_key: &'static str, model: impl Into<String>) -> Self {
        self.model_hint = Some(RuntimeModelHint::new(env_key, model));
        self
    }

    /// Keep the runtime/model env updates but leave the process-global
    /// initial-provider selection untouched.
    pub fn without_initial_selection(mut self) -> Self {
        if matches!(self.selection, RuntimeSelection::Initial(_)) {
            self.selection = RuntimeSelection::Unchanged;
        }
        self
    }

    pub fn initial(runtime_id: RuntimeProviderId, active_provider: ActiveProvider) -> Self {
        Self::new(runtime_id, RuntimeSelection::Initial(active_provider))
    }

    pub fn unlocked(runtime_id: RuntimeProviderId, active_hint: Option<ActiveProvider>) -> Self {
        Self::new(runtime_id, RuntimeSelection::Unlocked { active_hint })
    }

    pub fn azure_openai(model: Option<String>) -> Self {
        let activation = Self::initial(RuntimeProviderId::AzureOpenAi, ActiveProvider::OpenRouter);
        if let Some(model) = model.filter(|value| !value.trim().is_empty()) {
            activation.with_model_hint("JCODE_OPENROUTER_MODEL", model)
        } else {
            activation
        }
    }

    pub fn openai_compatible(model: Option<String>) -> Self {
        let activation = Self::initial(
            RuntimeProviderId::OpenAiCompatible,
            ActiveProvider::OpenRouter,
        );
        if let Some(model) = model.filter(|value| !value.trim().is_empty()) {
            activation.with_model_hint("JCODE_OPENROUTER_MODEL", model)
        } else {
            activation
        }
    }

    pub fn jcode_subscription(model: impl Into<String>) -> Self {
        Self::initial(RuntimeProviderId::Jcode, ActiveProvider::OpenRouter)
            .with_model_hint("JCODE_OPENROUTER_MODEL", model)
    }

    pub fn apply_env(&self) -> Result<()> {
        crate::env::set_var("JCODE_RUNTIME_PROVIDER", self.runtime_id.key());
        match self.runtime_id {
            RuntimeProviderId::Jcode => {
                crate::env::set_var("JCODE_OPENROUTER_TRANSPORT_STATE", "jcode-subscription")
            }
            RuntimeProviderId::OpenRouter => {
                crate::env::set_var("JCODE_OPENROUTER_TRANSPORT_STATE", "openrouter-api-key")
            }
            RuntimeProviderId::AzureOpenAi => {
                crate::env::set_var("JCODE_OPENROUTER_TRANSPORT_STATE", "direct-api-key")
            }
            RuntimeProviderId::OpenAiCompatible => {
                if std::env::var_os("JCODE_OPENROUTER_TRANSPORT_STATE").is_none() {
                    crate::env::set_var("JCODE_OPENROUTER_TRANSPORT_STATE", "direct-api-key");
                }
            }
            _ => {
                crate::env::remove_var("JCODE_OPENROUTER_TRANSPORT_STATE");
            }
        }

        let mut active_key_for_log = "";
        match self.selection {
            RuntimeSelection::Initial(active_provider) => {
                active_key_for_log = provider_key(active_provider);
                crate::env::set_var("JCODE_ACTIVE_PROVIDER", active_key_for_log);
                crate::env::set_var("JCODE_INITIAL_PROVIDER_EXPLICIT", "1");
            }
            RuntimeSelection::Unlocked { active_hint } => {
                crate::env::remove_var("JCODE_INITIAL_PROVIDER_EXPLICIT");
                if let Some(active_provider) = active_hint {
                    active_key_for_log = provider_key(active_provider);
                    crate::env::set_var("JCODE_ACTIVE_PROVIDER", active_key_for_log);
                } else {
                    crate::env::remove_var("JCODE_ACTIVE_PROVIDER");
                }
            }
            RuntimeSelection::Unchanged => {}
        }

        if let Some(model_hint) = &self.model_hint {
            crate::env::set_var(model_hint.env_key, &model_hint.model);
        }

        let model_env = self
            .model_hint
            .as_ref()
            .map(|hint| hint.env_key)
            .unwrap_or("");
        crate::logging::auth_event(
            "runtime_activation",
            self.runtime_id.key(),
            &[
                ("label", self.runtime_id.label()),
                ("selection", self.selection.log_value()),
                ("active_provider", active_key_for_log),
                ("model_env", model_env),
            ],
        );
        Ok(())
    }
}

/// Select the provider used when a new multi-provider runtime starts.
/// Later model switches remain free to select any configured provider.
pub fn select_initial_runtime_provider_key(provider_key_raw: &str) {
    crate::env::set_var("JCODE_RUNTIME_PROVIDER", provider_key_raw);
    crate::env::set_var("JCODE_ACTIVE_PROVIDER", provider_key_raw);
    crate::env::set_var("JCODE_INITIAL_PROVIDER_EXPLICIT", "1");
    crate::logging::auth_event(
        "runtime_activation_initial_provider",
        provider_key_raw,
        &[("selection", "initial")],
    );
}

pub fn clear_initial_runtime_provider() {
    crate::env::remove_var("JCODE_ACTIVE_PROVIDER");
    crate::env::remove_var("JCODE_INITIAL_PROVIDER_EXPLICIT");
    crate::logging::auth_event(
        "runtime_activation_clear_initial_provider",
        "runtime",
        &[("selection", "auto")],
    );
}

pub fn apply_azure_openai_runtime() -> Result<Option<String>> {
    apply_azure_openai_runtime_with(true)
}

/// `select_initial = false` is used by the shared daemon's auth-change path so
/// that configuring a provider in one session does not change the provider
/// that every future session starts on.
pub fn apply_azure_openai_runtime_with(select_initial: bool) -> Result<Option<String>> {
    crate::auth::azure::apply_runtime_env()?;
    let model = crate::auth::azure::load_model();
    let activation = ProviderActivation::azure_openai(model.clone());
    let activation = if select_initial {
        activation
    } else {
        activation.without_initial_selection()
    };
    activation.apply_env()?;
    Ok(model)
}

pub fn apply_openai_compatible_runtime(default_model: Option<String>) -> Result<()> {
    apply_openai_compatible_runtime_with(default_model, true)
}

pub fn apply_openai_compatible_runtime_with(
    default_model: Option<String>,
    select_initial: bool,
) -> Result<()> {
    let activation = ProviderActivation::openai_compatible(default_model);
    let activation = if select_initial {
        activation
    } else {
        activation.without_initial_selection()
    };
    activation.apply_env()
}

#[cfg(test)]
mod tests {
    use super::*;

    struct EnvGuard {
        saved: Vec<(&'static str, Option<String>)>,
    }

    impl EnvGuard {
        fn new(keys: &[&'static str]) -> Self {
            let saved = keys
                .iter()
                .map(|key| (*key, std::env::var(key).ok()))
                .collect();
            for key in keys {
                crate::env::remove_var(key);
            }
            Self { saved }
        }
    }

    impl Drop for EnvGuard {
        fn drop(&mut self) {
            for (key, value) in self.saved.drain(..) {
                if let Some(value) = value {
                    crate::env::set_var(key, value);
                } else {
                    crate::env::remove_var(key);
                }
            }
        }
    }

    #[test]
    fn openai_compatible_activation_without_initial_selection_keeps_default() {
        let _lock = crate::storage::lock_test_env();
        let _guard = EnvGuard::new(&[
            "JCODE_RUNTIME_PROVIDER",
            "JCODE_ACTIVE_PROVIDER",
            "JCODE_INITIAL_PROVIDER_EXPLICIT",
            "JCODE_OPENROUTER_MODEL",
            "JCODE_OPENROUTER_TRANSPORT_STATE",
        ]);
        crate::env::set_var("JCODE_ACTIVE_PROVIDER", "claude");

        apply_openai_compatible_runtime_with(Some("morph-v3-large".to_string()), false).unwrap();

        assert_eq!(
            std::env::var("JCODE_ACTIVE_PROVIDER").as_deref(),
            Ok("claude")
        );
        assert!(std::env::var_os("JCODE_INITIAL_PROVIDER_EXPLICIT").is_none());
        assert_eq!(
            std::env::var("JCODE_OPENROUTER_MODEL").as_deref(),
            Ok("morph-v3-large")
        );

        apply_openai_compatible_runtime_with(None, true).unwrap();
        assert_eq!(
            std::env::var("JCODE_ACTIVE_PROVIDER").as_deref(),
            Ok("openrouter")
        );
    }

    #[test]
    fn initial_provider_selection_preserves_raw_runtime_key() {
        let _guard = crate::storage::lock_test_env();
        let _env = EnvGuard::new(&[
            "JCODE_RUNTIME_PROVIDER",
            "JCODE_ACTIVE_PROVIDER",
            "JCODE_INITIAL_PROVIDER_EXPLICIT",
        ]);

        select_initial_runtime_provider_key("custom-provider-key");

        assert_eq!(
            std::env::var("JCODE_RUNTIME_PROVIDER").as_deref(),
            Ok("custom-provider-key")
        );
        assert_eq!(
            std::env::var("JCODE_ACTIVE_PROVIDER").as_deref(),
            Ok("custom-provider-key")
        );
    }

    #[test]
    fn azure_activation_preserves_identity_while_using_openrouter_slot() {
        // Serialize with every other test that mutates provider env vars
        // (e.g. anthropic_tests sets JCODE_RUNTIME_PROVIDER=claude); without
        // this lock the assertions below race parallel tests.
        let _lock = crate::storage::lock_test_env();
        let _guard = EnvGuard::new(&[
            "JCODE_RUNTIME_PROVIDER",
            "JCODE_ACTIVE_PROVIDER",
            "JCODE_INITIAL_PROVIDER_EXPLICIT",
            "JCODE_OPENROUTER_MODEL",
        ]);

        ProviderActivation::azure_openai(Some("gpt-4.1-mini".to_string()))
            .apply_env()
            .unwrap();

        assert_eq!(
            std::env::var("JCODE_RUNTIME_PROVIDER").as_deref(),
            Ok("azure-openai")
        );
        assert_eq!(
            std::env::var("JCODE_ACTIVE_PROVIDER").as_deref(),
            Ok("openrouter")
        );
        assert_eq!(
            std::env::var("JCODE_INITIAL_PROVIDER_EXPLICIT").as_deref(),
            Ok("1")
        );
        assert_eq!(
            std::env::var("JCODE_OPENROUTER_MODEL").as_deref(),
            Ok("gpt-4.1-mini")
        );
    }
}
