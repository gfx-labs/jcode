use super::*;
use crate::tui::backend::RemoteConnection;

pub(super) struct PendingGeneration {
    pub id: u64,
    pub revision: u64,
    pub session_id: String,
    pub started: Instant,
    pub remote_sent: bool,
    pub task: Option<tokio::task::JoinHandle<()>>,
}

impl Drop for PendingGeneration {
    fn drop(&mut self) {
        if let Some(task) = self.task.take() {
            task.abort();
        }
    }
}

pub(super) struct GenerationResult {
    pub id: u64,
    pub result: Result<crate::agent_instructions::GeneratedAgentInstructions>,
}

impl AgentWizard {
    pub(in crate::tui::app) fn cancel_generation(&mut self) {
        if let Some(generation) = self.generation.take() {
            if generation.remote_sent {
                self.pending_remote_cancel = Some(generation.id);
            }
        }
        self.generation_result = None;
    }

    fn generation_failed(&mut self, message: String) {
        self.cancel_generation();
        self.enter_step(Step::InstructionChoice);
        self.error = Some(format!(
            "{message} Retry Generate draft or choose Write myself. Existing instructions are intact."
        ));
    }
}

impl App {
    pub(in crate::tui::app) fn start_agent_wizard_generation(&mut self, wizard: &mut AgentWizard) {
        wizard.cancel_generation();
        wizard.enter_step(Step::Generating);
        let id = wizard.next_generation_id;
        wizard.next_generation_id = id.wrapping_add(1);
        let mut pending = PendingGeneration {
            id,
            revision: wizard.revision,
            session_id: self.session.id.clone(),
            started: Instant::now(),
            remote_sent: false,
            task: None,
        };
        if !self.is_remote {
            let Ok(runtime) = tokio::runtime::Handle::try_current() else {
                wizard.generation_failed("Generation needs an active runtime.".into());
                return;
            };
            let provider = match self.provider.fork_for_instruction_generation() {
                Ok(provider) => provider,
                Err(error) => {
                    wizard.generation_failed(error.to_string());
                    return;
                }
            };
            let description = wizard.draft.description.clone();
            let purpose = wizard.purpose.clone();
            let mode = wizard_mode_name(wizard.draft.mode).to_string();
            let (tx, rx) = mpsc::channel();
            pending.task = Some(runtime.spawn(async move {
                let result = crate::agent_instructions::generate_agent_instructions(
                    provider,
                    description,
                    purpose,
                    mode,
                )
                .await;
                let _ = tx.send(GenerationResult { id, result });
            }));
            wizard.generation_result = Some(rx);
        }
        wizard.generation = Some(pending);
    }

    pub(in crate::tui::app) fn finish_agent_wizard_generation(
        &mut self,
        id: u64,
        text: Option<String>,
        model: String,
        provider_name: String,
        error: Option<String>,
    ) -> bool {
        let Some(wizard) = self.agent_wizard.as_mut() else {
            return false;
        };
        let Some(pending) = wizard.generation.as_ref() else {
            return false;
        };
        if pending.id != id {
            return false;
        }
        if pending.session_id != self.session.id
            || wizard.session_id != self.session.id
            || pending.revision != wizard.revision
        {
            wizard.generation_failed(
                "Generation was discarded because the session or draft changed.".into(),
            );
            return true;
        }
        wizard.generation = None;
        wizard.generation_result = None;
        if let Some(error) = error {
            wizard.generation_failed(error);
        } else if let Some(text) =
            text.filter(|text| !text.trim().is_empty() && text.len() <= 65536)
        {
            wizard.draft.prompt = input::strip_terminal_control_sequences(&text).into_owned();
            wizard.generator_identity = format!("{provider_name} / {model}");
            wizard.enter_step(Step::Instructions);
        } else {
            wizard.generation_failed(
                "The generator returned empty or oversized instructions.".into(),
            );
        }
        true
    }

    pub(in crate::tui::app) fn poll_agent_wizard(&mut self) -> bool {
        let Some(wizard) = self.agent_wizard.as_mut() else {
            return false;
        };
        if wizard.session_id != self.session.id {
            wizard.cancel_generation();
            self.agent_wizard_remote_cancel = wizard.pending_remote_cancel.take();
            self.agent_wizard = None;
            self.set_status_notice(
                "Agent draft closed because the active session changed. Nothing was saved.",
            );
            return true;
        }
        if wizard
            .generation
            .as_ref()
            .is_some_and(|pending| pending.started.elapsed() >= Duration::from_secs(120))
        {
            wizard.generation_failed("Generation timed out. If using an older daemon, upgrade it to support instruction generation.".into());
            self.agent_wizard_remote_cancel = wizard.pending_remote_cancel.take();
            return true;
        }
        let clipboard_changed = wizard.poll_clipboard();
        let result = wizard
            .generation_result
            .as_ref()
            .and_then(|rx| rx.try_recv().ok());
        if let Some(result) = result {
            match result.result {
                Ok(generated) => self.finish_agent_wizard_generation(
                    result.id,
                    Some(generated.text),
                    generated.model,
                    generated.provider_name,
                    None,
                ),
                Err(error) => self.finish_agent_wizard_generation(
                    result.id,
                    None,
                    String::new(),
                    String::new(),
                    Some(error.to_string()),
                ),
            }
        } else {
            clipboard_changed
        }
    }

    pub(in crate::tui::app) fn agent_wizard_disconnected(&mut self) {
        if let Some(wizard) = self.agent_wizard.as_mut() {
            if wizard.generation.is_some() {
                wizard.generation_failed(
                    "Disconnected. Generation was canceled and will not retry automatically."
                        .into(),
                );
            }
            wizard.pending_remote_cancel = None;
        }
        self.agent_wizard_remote_cancel = None;
    }

    pub(in crate::tui::app) fn agent_wizard_protocol_error(
        &mut self,
        id: u64,
        message: &str,
    ) -> bool {
        if self
            .agent_wizard
            .as_ref()
            .and_then(|wizard| wizard.generation.as_ref())
            .is_some_and(|pending| pending.remote_sent && pending.id == id)
        {
            self.finish_agent_wizard_generation(
                id,
                None,
                String::new(),
                String::new(),
                Some(format!(
                    "Generation failed: {message}. An older daemon may require an upgrade."
                )),
            )
        } else {
            false
        }
    }

    pub(in crate::tui::app) async fn dispatch_agent_wizard_generation(
        &mut self,
        remote: &mut RemoteConnection,
    ) -> bool {
        if let Some(id) = self.agent_wizard_remote_cancel.take() {
            let _ = remote.cancel_agent_instructions(id).await;
        }
        let Some(wizard) = self.agent_wizard.as_mut() else {
            return false;
        };
        if let Some(id) = wizard.pending_remote_cancel.take() {
            let _ = remote.cancel_agent_instructions(id).await;
        }
        let Some(pending) = wizard.generation.as_mut() else {
            return false;
        };
        if pending.remote_sent {
            return false;
        }
        match remote
            .generate_agent_instructions(
                wizard.draft.description.clone(),
                wizard.purpose.clone(),
                wizard_mode_name(wizard.draft.mode).into(),
            )
            .await
        {
            Ok(id) => {
                pending.id = id;
                pending.remote_sent = true;
            }
            Err(error) => {
                wizard.generation_failed(format!("Could not request generation: {error}"))
            }
        }
        true
    }
}

pub(in crate::tui::app) fn wizard_mode_name(mode: AgentMode) -> &'static str {
    match mode {
        AgentMode::All => "all",
        AgentMode::Primary => "primary",
        AgentMode::Subagent => "subagent",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pending_app() -> App {
        let mut app = crate::tui::app::tests::create_test_app();
        app.is_remote = true;
        app.open_agent_wizard();
        let mut wizard = app.agent_wizard.take().unwrap();
        wizard.purpose = "Review code".into();
        wizard.draft.description = "Reviewer".into();
        app.start_agent_wizard_generation(&mut wizard);
        app.agent_wizard = Some(wizard);
        app
    }

    #[test]
    fn agent_wizard_timeout_at_120_seconds_preserves_existing_instructions() {
        let mut app = pending_app();
        let wizard = app.agent_wizard.as_mut().unwrap();
        wizard.draft.prompt = "Keep existing edits".into();
        wizard.generation.as_mut().unwrap().started = Instant::now() - Duration::from_secs(121);
        assert!(app.poll_agent_wizard());
        let wizard = app.agent_wizard.as_ref().unwrap();
        assert_eq!(wizard.step, Step::InstructionChoice);
        assert_eq!(wizard.draft.prompt, "Keep existing edits");
        assert!(wizard.error.as_deref().unwrap().contains("timed out"));
    }

    #[test]
    fn agent_wizard_stale_revision_and_wrong_ids_never_replace_instructions() {
        let mut app = pending_app();
        let wizard = app.agent_wizard.as_mut().unwrap();
        let id = wizard.generation.as_ref().unwrap().id;
        wizard.draft.prompt = "Keep edits".into();
        assert!(!app.finish_agent_wizard_generation(
            id + 1,
            Some("Wrong id".into()),
            "model".into(),
            "provider".into(),
            None
        ));
        app.agent_wizard.as_mut().unwrap().revision += 1;
        app.finish_agent_wizard_generation(
            id,
            Some("Stale revision".into()),
            "model".into(),
            "provider".into(),
            None,
        );
        assert_eq!(
            app.agent_wizard.as_ref().unwrap().draft.prompt,
            "Keep edits"
        );
        assert_eq!(
            app.agent_wizard.as_ref().unwrap().step,
            Step::InstructionChoice
        );
    }
}
