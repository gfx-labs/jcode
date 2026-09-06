use super::*;

struct RegisteredSessionProvider {
    token: u64,
    provider: std::sync::Weak<dyn Provider>,
}

static SESSION_PROVIDERS: std::sync::LazyLock<
    std::sync::Mutex<std::collections::HashMap<String, RegisteredSessionProvider>>,
> = std::sync::LazyLock::new(|| std::sync::Mutex::new(std::collections::HashMap::new()));
static NEXT_PROVIDER_REGISTRATION: std::sync::atomic::AtomicU64 =
    std::sync::atomic::AtomicU64::new(1);

/// RAII-owned weak lookup. It cannot keep a closed Agent/provider alive, and
/// dropping an older attachment cannot erase a newer registration of the ID.
pub(super) struct SessionProviderRegistration {
    session_id: String,
    token: u64,
}

impl SessionProviderRegistration {
    pub(super) fn new(session_id: &str, provider: &Arc<dyn Provider>) -> Self {
        let token = NEXT_PROVIDER_REGISTRATION.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        SESSION_PROVIDERS
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .insert(
                session_id.to_string(),
                RegisteredSessionProvider {
                    token,
                    provider: Arc::downgrade(provider),
                },
            );
        Self {
            session_id: session_id.to_string(),
            token,
        }
    }
}

impl Drop for SessionProviderRegistration {
    fn drop(&mut self) {
        let mut providers = SESSION_PROVIDERS.lock().unwrap_or_else(|e| e.into_inner());
        if providers
            .get(&self.session_id)
            .is_some_and(|entry| entry.token == self.token)
        {
            providers.remove(&self.session_id);
        }
    }
}

impl Agent {
    /// The connection's original provider may belong to a different session
    /// after resume. Metadata fallback must resolve the live target provider.
    pub(crate) fn provider_handle_for_session(session_id: &str) -> Option<Arc<dyn Provider>> {
        SESSION_PROVIDERS
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .get(session_id)
            .and_then(|entry| entry.provider.upgrade())
    }

    pub(super) fn refresh_provider_registration(&mut self) {
        self._provider_registration =
            SessionProviderRegistration::new(&self.session.id, &self.provider);
    }

    /// Snapshot the worker-only OpenAI policy. Keep `off` explicit: `None`
    /// means preserve the provider default, not disable priority.
    pub(crate) fn configured_spawn_openai_service_tier() -> Result<Option<String>> {
        let config = crate::config::config();
        normalize_spawn_openai_service_tier(config.agents.swarm_openai_service_tier.as_deref())
    }

    pub(crate) fn initialize_spawn_openai_service_tier(&mut self) -> Result<()> {
        self.session.spawn_openai_service_tier = Self::configured_spawn_openai_service_tier()?;
        self.restore_spawn_openai_service_tier()?;
        self.session.save()?;
        Ok(())
    }

    /// Apply only an explicitly persisted worker policy, and only to OpenAI.
    /// Parent IDs cannot identify workers: normal user forks also have parents.
    pub(crate) fn restore_spawn_openai_service_tier(&self) -> Result<()> {
        if self.provider.name().eq_ignore_ascii_case("openai")
            && let Some(tier) = self.session.spawn_openai_service_tier.as_deref()
        {
            self.provider.set_service_tier(tier)?;
        }
        Ok(())
    }

    /// Clear the active worker override before parking an OpenAI provider slot
    /// or changing sessions. A later main session must not inherit that slot's
    /// worker-only tier. Invalid main defaults behave like the OpenAI loader:
    /// warn there and fall back to standard service here.
    pub(crate) fn reset_spawn_openai_service_tier(&self) -> Result<()> {
        if self.session.spawn_openai_service_tier.is_some()
            && self.provider.name().eq_ignore_ascii_case("openai")
        {
            let config = crate::config::config();
            let tier =
                normalize_spawn_openai_service_tier(config.provider.openai_service_tier.as_deref())
                    .ok()
                    .flatten();
            self.provider
                .set_service_tier(tier.as_deref().unwrap_or("off"))?;
        }
        Ok(())
    }

    pub fn set_premium_mode(&self, mode: crate::provider::copilot::PremiumMode) {
        self.provider.set_premium_mode(mode);
    }

    pub fn premium_mode(&self) -> crate::provider::copilot::PremiumMode {
        self.provider.premium_mode()
    }

    pub fn provider_fork(&self) -> Arc<dyn Provider> {
        self.provider.fork()
    }

    pub fn provider_handle(&self) -> Arc<dyn Provider> {
        Arc::clone(&self.provider)
    }

    pub fn available_models(&self) -> Vec<&'static str> {
        self.provider.available_models()
    }

    pub fn available_models_for_switching(&self) -> Vec<String> {
        self.provider.available_models_for_switching()
    }

    pub fn available_models_display(&self) -> Vec<String> {
        self.provider.available_models_display()
    }

    pub fn model_routes(&self) -> Vec<crate::provider::ModelRoute> {
        self.provider.model_routes()
    }

    pub fn model_catalog_snapshot(&self) -> jcode_provider_core::ModelCatalogSnapshot {
        jcode_provider_core::ModelCatalogSnapshot::new(
            Some(self.provider_name()),
            Some(self.provider_model()),
            self.available_models_display(),
            self.model_routes(),
        )
    }

    pub fn registry(&self) -> Registry {
        self.registry.clone()
    }

    pub async fn compaction_mode(&self) -> crate::config::CompactionMode {
        self.registry.compaction().read().await.mode()
    }

    pub async fn set_compaction_mode(&self, mode: crate::config::CompactionMode) -> Result<()> {
        let compaction = self.registry.compaction();
        let mut manager = compaction.write().await;
        manager.set_mode(mode);
        Ok(())
    }

    fn refresh_compaction_budget(&self) {
        let compaction = self.registry.compaction();
        match compaction.try_write() {
            Ok(mut manager) => manager.set_budget(self.provider.context_window()),
            Err(_) => crate::logging::warn(
                "Could not refresh compaction token budget after provider change: compaction manager is busy",
            ),
        }
    }

    #[cfg(test)]
    pub(crate) async fn compaction_token_budget(&self) -> usize {
        self.registry.compaction().read().await.token_budget()
    }

    pub fn provider_messages(&mut self) -> Vec<Message> {
        self.session.messages_for_provider()
    }

    pub fn set_model(&mut self, model: &str) -> Result<()> {
        self.set_model_from_provider_state_event(
            model,
            crate::provider::ProviderModelSelectionSource::User,
        )
    }

    pub fn set_route_selection(
        &mut self,
        selection: &crate::provider::RouteSelection,
    ) -> Result<()> {
        self.set_route_selection_from_provider_state_event(
            selection,
            crate::provider::ProviderModelSelectionSource::User,
        )
    }

    pub(crate) fn set_route_selection_from_auth(
        &mut self,
        selection: &crate::provider::RouteSelection,
    ) -> Result<()> {
        self.set_route_selection_from_provider_state_event(
            selection,
            crate::provider::ProviderModelSelectionSource::Auth,
        )
    }

    fn set_route_selection_from_provider_state_event(
        &mut self,
        selection: &crate::provider::RouteSelection,
        source: crate::provider::ProviderModelSelectionSource,
    ) -> Result<()> {
        self.reset_spawn_openai_service_tier()?;
        let switch_result = self.provider.set_route_selection(selection);
        self.restore_spawn_openai_service_tier()?;
        switch_result?;
        let resolved_model = self.provider.model();
        self.session.provider_key = Some(selection.runtime_key.stable_id());
        self.session.route_api_method = Some(selection.api_method.clone());
        self.session.model = Some(self.provider_model());
        let event = crate::provider::ProviderStateEvent::selected_model(source, resolved_model);
        self.provider_runtime_state.apply(event);
        self.refresh_compaction_budget();
        self.persist_session_best_effort("route selection");
        self.log_env_snapshot("set_route_selection");
        Ok(())
    }

    pub(crate) fn set_model_from_auth(&mut self, model: &str) -> Result<()> {
        self.set_model_from_provider_state_event(
            model,
            crate::provider::ProviderModelSelectionSource::Auth,
        )
    }

    fn set_model_from_provider_state_event(
        &mut self,
        model: &str,
        source: crate::provider::ProviderModelSelectionSource,
    ) -> Result<()> {
        self.reset_spawn_openai_service_tier()?;
        let switch_result =
            crate::provider::set_model_with_auth_refresh(self.provider.as_ref(), model);
        self.restore_spawn_openai_service_tier()?;
        switch_result?;
        let resolved_model = self.provider.model();
        self.session.provider_key =
            crate::provider::MultiProvider::session_provider_key_after_model_switch(
                model,
                self.provider.name(),
                self.session.provider_key.as_deref(),
            );
        self.session.model = Some(self.provider_model());
        let event = crate::provider::ProviderStateEvent::selected_model(source, resolved_model);
        self.provider_runtime_state.apply(event);
        self.refresh_compaction_budget();
        self.persist_session_best_effort("model selection");
        self.log_env_snapshot("set_model");
        Ok(())
    }

    pub(crate) fn provider_model_selection_generation(&self) -> u64 {
        self.provider_runtime_state.selection_generation()
    }

    pub(crate) fn user_selected_provider_model_after(&self, generation: u64) -> bool {
        self.provider_runtime_state.user_selected_after(generation)
    }

    pub fn restore_reasoning_effort_from_session(&mut self) {
        if let Some(effort) = self.session.reasoning_effort.clone() {
            if let Err(e) = self.provider.set_reasoning_effort(&effort) {
                crate::logging::error(&format!(
                    "Failed to restore session reasoning effort '{}': {}",
                    effort, e
                ));
            }
        } else {
            self.session.reasoning_effort = self.provider.reasoning_effort();
        }
        // Mirror the effort into the deadlock-free side-table so server handlers
        // (e.g. the swarm seed handler) can learn this session's effort without
        // taking the agent lock.
        crate::session_effort::record_session_effort(
            &self.session.id,
            self.session.reasoning_effort.as_deref(),
        );
    }

    pub fn set_reasoning_effort(&mut self, effort: &str) -> Result<Option<String>> {
        self.provider.set_reasoning_effort(effort)?;
        let current = self.provider.reasoning_effort();
        self.session.reasoning_effort = current.clone();
        // Keep the side-table in sync (see `restore_reasoning_effort_from_session`).
        crate::session_effort::record_session_effort(&self.session.id, current.as_deref());
        self.log_env_snapshot("set_reasoning_effort");
        self.session.save()?;
        Ok(current)
    }

    pub fn subagent_model(&self) -> Option<String> {
        self.session.subagent_model.clone()
    }

    pub fn set_subagent_model(&mut self, model: Option<String>) -> Result<()> {
        self.session.subagent_model = model;
        self.log_env_snapshot("set_subagent_model");
        self.session.save()?;
        Ok(())
    }

    pub fn session_provider_key(&self) -> Option<String> {
        self.session.provider_key.clone()
    }

    /// API method/runtime route used to select the active model (e.g.
    /// "openai-api", "claude-oauth", "openai-compatible:nvidia-nim"). Spawned
    /// swarm agents inherit this so they reconstruct the coordinator's exact
    /// auth route instead of falling back to the config default.
    pub fn session_route_api_method(&self) -> Option<String> {
        self.session.route_api_method.clone()
    }

    /// The credential the active provider will use for the next request, when
    /// the provider distinguishes OAuth (subscription) from API key (cost).
    /// Resolved authoritatively here so remote clients can render billing/usage
    /// without re-deriving it from the provider name.
    pub fn active_resolved_credential(&self) -> Option<jcode_provider_core::ResolvedCredential> {
        self.provider.active_resolved_credential()
    }

    pub fn set_session_provider_key(&mut self, provider_key: Option<String>) {
        self.session.provider_key = provider_key;
    }

    pub fn rename_session_title(&mut self, title: Option<String>) -> Result<String> {
        self.session.rename_title(title);
        self.log_env_snapshot("rename_session");
        self.session.save()?;
        Ok(self.session.display_title_or_name().to_string())
    }

    pub fn autoreview_enabled(&self) -> Option<bool> {
        self.session.autoreview_enabled
    }

    pub fn set_autoreview_enabled(&mut self, enabled: bool) -> Result<()> {
        self.session.autoreview_enabled = Some(enabled);
        self.log_env_snapshot("set_autoreview_enabled");
        self.session.save()?;
        Ok(())
    }

    pub fn autojudge_enabled(&self) -> Option<bool> {
        self.session.autojudge_enabled
    }

    pub fn set_autojudge_enabled(&mut self, enabled: bool) -> Result<()> {
        self.session.autojudge_enabled = Some(enabled);
        self.log_env_snapshot("set_autojudge_enabled");
        self.session.save()?;
        Ok(())
    }

    /// Set the working directory for this session
    pub fn set_working_dir(&mut self, dir: &str) {
        if self.session.working_dir.as_deref() == Some(dir) {
            return;
        }
        self.session.working_dir = Some(dir.to_string());
        self.refresh_agents_md_snapshot();
        self.session.refresh_initial_session_context_message();
        self.log_env_snapshot("working_dir");
    }

    /// Get the working directory for this session
    pub fn working_dir(&self) -> Option<&str> {
        self.session.working_dir.as_deref()
    }

    /// Get the stored messages (for transcript export)
    pub fn messages(&self) -> &[StoredMessage] {
        &self.session.messages
    }
}

fn normalize_spawn_openai_service_tier(raw: Option<&str>) -> Result<Option<String>> {
    let Some(value) = raw.map(str::trim).filter(|value| !value.is_empty()) else {
        return Ok(None);
    };
    match value.to_ascii_lowercase().as_str() {
        "inherit" => Ok(None),
        "priority" | "fast" => Ok(Some("priority".to_string())),
        "flex" => Ok(Some("flex".to_string())),
        "off" | "standard" | "default" | "auto" | "none" => Ok(Some("off".to_string())),
        _ => anyhow::bail!(
            "Invalid agents.swarm_openai_service_tier '{value}': use priority, flex, off, or inherit"
        ),
    }
}
