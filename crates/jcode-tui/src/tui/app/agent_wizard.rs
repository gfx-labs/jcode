use super::*;
use crate::agent_profile::{AgentMode, AgentProfileDraft, AgentProfileScope};
use crate::tui::core::{next_char_boundary, prev_char_boundary};

mod generation;
mod render;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Step {
    Location,
    Name,
    Description,
    Purpose,
    Mode,
    InstructionChoice,
    Instructions,
    Model,
    Effort,
    Skills,
    Review,
    DiscardConfirmation,
    RegenerateConfirmation,
    Generating,
}

impl Step {
    fn previous(self) -> Self {
        match self {
            Self::Name => Self::Location,
            Self::Description => Self::Name,
            Self::Purpose => Self::Description,
            Self::Mode => Self::Purpose,
            Self::InstructionChoice => Self::Mode,
            Self::Instructions | Self::Generating | Self::RegenerateConfirmation => {
                Self::InstructionChoice
            }
            Self::Model => Self::Instructions,
            Self::Effort => Self::Model,
            Self::Skills => Self::Effort,
            Self::Review => Self::Skills,
            _ => Self::Location,
        }
    }
}

pub struct AgentWizard {
    pub(super) step: Step,
    pub(super) draft: AgentProfileDraft,
    scope: AgentProfileScope,
    working_dir: Option<PathBuf>,
    purpose: String,
    editor: String,
    cursor: usize,
    undo: Vec<(String, usize)>,
    selected: usize,
    scroll: usize,
    detail_rows: std::cell::Cell<usize>,
    error: Option<String>,
    warning: Option<String>,
    acknowledged_warning: Option<String>,
    return_step: Step,
    models: Vec<WizardModel>,
    skills: Vec<String>,
    generator_identity: String,
    revision: u64,
    generation: Option<generation::PendingGeneration>,
    generation_result: Option<mpsc::Receiver<generation::GenerationResult>>,
    next_generation_id: u64,
    pending_remote_cancel: Option<u64>,
    session_id: String,
    clipboard: Option<ClipboardRead>,
}

struct ClipboardRead {
    revision: u64,
    receiver: mpsc::Receiver<Option<String>>,
}

struct WizardModel {
    spec: String,
    label: String,
    efforts: Vec<String>,
}

impl AgentWizard {
    fn text_step(&self) -> bool {
        matches!(
            self.step,
            Step::Name
                | Step::Description
                | Step::Purpose
                | Step::Instructions
                | Step::Model
                | Step::Skills
        )
    }

    fn multiline(&self) -> bool {
        matches!(self.step, Step::Purpose | Step::Instructions)
    }

    fn enter_step(&mut self, step: Step) {
        self.clipboard = None;
        self.step = step;
        self.selected = 0;
        self.scroll = 0;
        self.error = None;
        self.undo.clear();
        self.editor = match step {
            Step::Name => self.draft.name.clone(),
            Step::Description => self.draft.description.clone(),
            Step::Purpose => self.purpose.clone(),
            Step::Instructions => self.draft.prompt.clone(),
            _ => String::new(),
        };
        self.cursor = self.editor.len();
        self.selected = match step {
            Step::Location => usize::from(self.scope == AgentProfileScope::Global),
            Step::Mode => match self.draft.mode {
                AgentMode::All => 0,
                AgentMode::Primary => 1,
                AgentMode::Subagent => 2,
            },
            Step::Model => self
                .draft
                .model
                .as_ref()
                .and_then(|spec| self.models.iter().position(|model| &model.spec == spec))
                .unwrap_or(0),
            Step::Effort => self
                .draft
                .effort
                .as_ref()
                .and_then(|effort| self.efforts().iter().position(|item| item == effort))
                .map(|i| i + 1)
                .unwrap_or(0),
            _ => 0,
        };
    }

    fn store_editor(&mut self) {
        match self.step {
            Step::Name => self.draft.name = self.editor.clone(),
            Step::Description => self.draft.description = self.editor.clone(),
            Step::Purpose => self.purpose = self.editor.clone(),
            Step::Instructions => self.draft.prompt = self.editor.clone(),
            _ => {}
        }
    }

    fn insert(&mut self, text: &str) {
        if !self.text_step() {
            return;
        }
        let sanitized = input::strip_terminal_control_sequences(text);
        let text = if self.multiline() {
            sanitized.replace("\r\n", "\n").replace('\r', "\n")
        } else {
            sanitized.replace(['\r', '\n'], " ")
        };
        let max = match self.step {
            Step::Name => 64,
            Step::Description => 4000,
            Step::Purpose => 4096,
            Step::Instructions => 65536,
            _ => 1000,
        };
        if self.editor.len().saturating_add(text.len()) > max {
            self.error = Some(format!(
                "This field is limited to {max} bytes. Nothing was inserted."
            ));
            return;
        }
        self.remember_edit();
        self.editor.insert_str(self.cursor, &text);
        self.cursor += text.len();
        self.revision = self.revision.wrapping_add(1);
        self.selected = 0;
        self.error = None;
    }

    fn remember_edit(&mut self) {
        if self.undo.len() >= 32 {
            self.undo.remove(0);
        }
        self.undo.push((self.editor.clone(), self.cursor));
    }

    fn model_indices(&self) -> Vec<usize> {
        let query = self.editor.to_lowercase();
        self.models
            .iter()
            .enumerate()
            .filter(|(_, model)| model.label.to_lowercase().contains(&query))
            .map(|(i, _)| i)
            .collect()
    }

    fn skill_indices(&self) -> Vec<usize> {
        let query = self.editor.to_lowercase();
        self.skills
            .iter()
            .enumerate()
            .filter(|(_, name)| name.to_lowercase().contains(&query))
            .map(|(i, _)| i)
            .collect()
    }

    fn efforts(&self) -> &[String] {
        self.models
            .iter()
            .find(|model| Some(&model.spec) == self.draft.model.as_ref())
            .or_else(|| self.models.first())
            .map(|model| model.efforts.as_slice())
            .unwrap_or_default()
    }

    fn choices(&self) -> Vec<String> {
        match self.step {
            Step::Location => vec![
                if self.working_dir.is_some() {
                    "Project: this working directory".into()
                } else {
                    "Project unavailable: session has no working directory".into()
                },
                "Global: all projects".into(),
            ],
            Step::Mode => vec![
                "Main chat and workers (all)".into(),
                "Main chat only (primary)".into(),
                "Workers only (subagent)".into(),
            ],
            Step::InstructionChoice => vec![
                "Generate draft".into(),
                "Write myself (no network required)".into(),
            ],
            Step::Model => std::iter::once("Inherit the active model".into())
                .chain(
                    self.model_indices()
                        .into_iter()
                        .filter(|i| *i > 0)
                        .map(|i| self.models[i].label.clone()),
                )
                .collect(),
            Step::Effort => std::iter::once("No effort override".into())
                .chain(self.efforts().iter().cloned())
                .collect(),
            Step::Skills => self
                .skill_indices()
                .iter()
                .map(|i| {
                    format!(
                        "[{}] {}",
                        if self.draft.skills.contains(&self.skills[*i]) {
                            "x"
                        } else {
                            " "
                        },
                        self.skills[*i]
                    )
                })
                .collect(),
            Step::Review => vec![
                "Save".into(),
                if self.draft.mode.allows_primary() {
                    "Save and activate".into()
                } else {
                    "Save and activate unavailable: workers only".into()
                },
                "Edit from name".into(),
                "Cancel".into(),
            ],
            Step::DiscardConfirmation => vec!["Keep editing".into(), "Discard draft".into()],
            Step::RegenerateConfirmation => vec![
                "Keep existing instructions".into(),
                "Replace instructions with a new draft".into(),
            ],
            _ => Vec::new(),
        }
    }

    fn validate_name(&mut self) -> bool {
        match crate::agent_profile::profile_destination(
            self.scope,
            self.working_dir.as_deref(),
            &self.draft.name,
        ) {
            Err(error) => {
                self.error = Some(error.to_string());
                false
            }
            Ok(path) => {
                if std::fs::symlink_metadata(&path).is_ok() {
                    self.error = Some(format!(
                        "Already exists: {}. Choose another name. Nothing will be overwritten.",
                        path.display()
                    ));
                    return false;
                }
                let mut warnings = Vec::new();
                if self.scope == AgentProfileScope::Global && self.working_dir.is_some() {
                    if let Ok(project) = crate::agent_profile::profile_destination(
                        AgentProfileScope::Project,
                        self.working_dir.as_deref(),
                        &self.draft.name,
                    ) {
                        if std::fs::symlink_metadata(&project).is_ok() {
                            self.error = Some(format!(
                                "A project profile at {} would hide this global profile. Choose a unique name or another location.",
                                project.display()
                            ));
                            return false;
                        }
                    }
                }
                if self.scope == AgentProfileScope::Project {
                    if let Ok(global) = crate::agent_profile::profile_destination(
                        AgentProfileScope::Global,
                        None,
                        &self.draft.name,
                    ) {
                        if std::fs::symlink_metadata(global).is_ok() {
                            warnings.push(
                                "This project profile will shadow a global profile.".to_string(),
                            );
                        }
                    }
                }
                if matches!(self.draft.name.as_str(), "create" | "use" | "clear")
                    || commands::parse_agents_target(&self.draft.name).is_some()
                    || super::state_ui_input_helpers::registered_command_entries()
                        .any(|(name, _)| name.trim_start_matches('/') == self.draft.name)
                {
                    warnings.push(format!(
                        "Command-name collision. Use /agents use {} to activate it.",
                        self.draft.name
                    ));
                }
                let warning = warnings.join(" ");
                let acknowledgement = format!("{}: {warning}", path.display());
                if !warning.is_empty()
                    && self.acknowledged_warning.as_deref() != Some(&acknowledgement)
                {
                    self.warning = Some(format!("{warning} Press Enter again to acknowledge."));
                    self.acknowledged_warning = Some(acknowledgement);
                    return false;
                }
                self.warning = None;
                true
            }
        }
    }
}

impl App {
    pub(super) fn open_agent_wizard(&mut self) {
        if commands_dispatch::ssh_local_action_blocked(self, "Agent profile creation") {
            return;
        }
        if self.agent_wizard.is_some() {
            return;
        }
        if self.is_processing
            || self.pending_turn
            || self.agent_profile_switch_pending()
            || self.remote_model_switch_in_flight
            || self.pending_model_switch.is_some()
            || self.pending_route_selection.is_some()
            || self.submit_input_on_startup
        {
            self.set_status_notice(
                "Wait for the current turn or profile/model change before creating an agent.",
            );
            return;
        }
        let working_dir = self.session.working_dir.as_ref().map(PathBuf::from);
        let scope = if working_dir.is_some() {
            AgentProfileScope::Project
        } else {
            AgentProfileScope::Global
        };
        let (provider_name, model) = if self.is_remote {
            (
                self.remote_provider_name
                    .clone()
                    .unwrap_or_else(|| "remote provider".into()),
                self.remote_provider_model
                    .clone()
                    .unwrap_or_else(|| "unknown".into()),
            )
        } else {
            (self.provider.name().to_string(), self.provider.model())
        };
        let current_efforts = if self.is_remote {
            inferred_reasoning_efforts(Some(&provider_name), Some(&model))
        } else {
            self.provider.available_efforts()
        };
        let mut models = vec![WizardModel {
            spec: String::new(),
            label: "Inherit".into(),
            efforts: current_efforts.into_iter().map(str::to_owned).collect(),
        }];
        let routes = if self.is_remote {
            self.remote_model_options.clone()
        } else {
            self.provider.model_routes()
        };
        for route in routes {
            if !route.available {
                continue;
            }
            let option = crate::tui::PickerOption {
                provider: route.provider.clone(),
                api_method: route.api_method.clone(),
                available: true,
                detail: route.detail,
                estimated_reference_cost_micros: None,
            };
            let spec = super::inline_interactive::helpers::agent_route_model_spec(
                route.model.clone(),
                &option,
            );
            if models.iter().any(|model| model.spec == spec) {
                continue;
            }
            models.push(WizardModel {
                spec,
                label: format!(
                    "{} via {} ({})",
                    route.model, route.provider, route.api_method
                ),
                efforts: inferred_reasoning_efforts(Some(&route.provider), Some(&route.model))
                    .into_iter()
                    .map(str::to_owned)
                    .collect(),
            });
        }
        let registry = SkillRegistry::load_for_working_dir_read_only(working_dir.as_deref());
        let (skills, error) = match registry {
            Ok(registry) => (
                registry
                    .list()
                    .iter()
                    .map(|skill| skill.name.clone())
                    .collect(),
                None,
            ),
            Err(error) => (
                Vec::new(),
                Some(format!(
                    "Could not load skills: {error}. Manual instructions remain available."
                )),
            ),
        };
        self.inline_interactive_state = None;
        self.inline_view_state = None;
        self.agent_wizard = Some(AgentWizard {
            step: Step::Location,
            draft: AgentProfileDraft {
                name: String::new(),
                description: String::new(),
                mode: AgentMode::All,
                model: None,
                effort: None,
                skills: Vec::new(),
                prompt: String::new(),
            },
            scope,
            working_dir,
            purpose: String::new(),
            editor: String::new(),
            cursor: 0,
            undo: Vec::new(),
            selected: usize::from(scope == AgentProfileScope::Global),
            scroll: 0,
            detail_rows: std::cell::Cell::new(8),
            error,
            warning: None,
            acknowledged_warning: None,
            return_step: Step::Location,
            models,
            skills,
            generator_identity: format!("{provider_name} / {model}"),
            revision: 0,
            generation: None,
            generation_result: None,
            next_generation_id: 1,
            pending_remote_cancel: None,
            session_id: self.session.id.clone(),
            clipboard: None,
        });
    }

    pub(super) fn handle_agent_wizard_paste(&mut self, text: &str) -> bool {
        if self
            .agent_wizard
            .as_ref()
            .is_some_and(|wizard| wizard.session_id != self.session.id)
        {
            self.poll_agent_wizard();
            return true;
        }
        let Some(wizard) = self.agent_wizard.as_mut() else {
            return false;
        };
        wizard.insert(text);
        true
    }

    pub(super) fn handle_agent_wizard_key(
        &mut self,
        code: KeyCode,
        modifiers: KeyModifiers,
        text: Option<&str>,
    ) -> bool {
        if self
            .agent_wizard
            .as_ref()
            .is_some_and(|wizard| wizard.session_id != self.session.id)
        {
            self.poll_agent_wizard();
            return true;
        }
        let Some(mut wizard) = self.agent_wizard.take() else {
            return false;
        };
        let mut keep = true;
        if code == KeyCode::Esc {
            if wizard.step == Step::Generating {
                wizard.cancel_generation();
                wizard.enter_step(Step::InstructionChoice);
            } else if wizard.step == Step::DiscardConfirmation {
                wizard.enter_step(wizard.return_step);
            } else if wizard.step == Step::Location {
                wizard.return_step = wizard.step;
                wizard.enter_step(Step::DiscardConfirmation);
            } else {
                wizard.store_editor();
                wizard.enter_step(wizard.step.previous());
            }
        } else if modifiers.contains(KeyModifiers::CONTROL) && code == KeyCode::Char('c') {
            if wizard.step != Step::DiscardConfirmation {
                wizard.store_editor();
                wizard.return_step = if wizard.step == Step::Generating {
                    Step::InstructionChoice
                } else {
                    wizard.step
                };
                wizard.cancel_generation();
                wizard.enter_step(Step::DiscardConfirmation);
            }
        } else if wizard.step == Step::Generating {
            // Input remains private while the isolated request is in flight.
        } else if matches!(code, KeyCode::Char('v' | 'V'))
            && modifiers.intersects(
                KeyModifiers::CONTROL
                    | KeyModifiers::ALT
                    | KeyModifiers::SUPER
                    | KeyModifiers::META,
            )
            && wizard.text_step()
        {
            wizard.start_clipboard();
        } else if code == KeyCode::Enter
            && !modifiers.intersects(KeyModifiers::SHIFT | KeyModifiers::ALT)
        {
            wizard.store_editor();
            keep = self.advance_agent_wizard(&mut wizard);
        } else if code == KeyCode::Enter && wizard.multiline() {
            wizard.insert("\n");
        } else if matches!(code, KeyCode::PageUp | KeyCode::PageDown) {
            let page = wizard.detail_rows.get().max(1);
            wizard.scroll = if code == KeyCode::PageUp {
                wizard.scroll.saturating_sub(page)
            } else {
                wizard.scroll.saturating_add(page)
            };
        } else if matches!(
            code,
            KeyCode::Up | KeyCode::Down | KeyCode::Tab | KeyCode::BackTab
        ) && !wizard.choices().is_empty()
        {
            let count = wizard.choices().len();
            wizard.selected = if matches!(code, KeyCode::Up | KeyCode::BackTab) {
                wizard.selected.checked_sub(1).unwrap_or(count - 1)
            } else {
                (wizard.selected + 1) % count
            };
        } else if code == KeyCode::Char(' ') && wizard.step == Step::Skills {
            if let Some(index) = wizard.skill_indices().get(wizard.selected) {
                let name = wizard.skills[*index].clone();
                if wizard.draft.skills.contains(&name) {
                    wizard.draft.skills.retain(|skill| skill != &name);
                } else {
                    wizard.draft.skills.push(name);
                }
                wizard.revision = wizard.revision.wrapping_add(1);
            }
        } else if wizard.text_step() {
            wizard.edit_key(code, modifiers, text);
        }
        if let Some(id) = wizard.pending_remote_cancel.take() {
            self.agent_wizard_remote_cancel = Some(id);
        }
        if keep {
            self.agent_wizard = Some(wizard);
        }
        true
    }

    fn advance_agent_wizard(&mut self, wizard: &mut AgentWizard) -> bool {
        match wizard.step {
            Step::Location => {
                if wizard.selected == 0 && wizard.working_dir.is_none() {
                    wizard.error =
                        Some("Project requires a session working directory. Choose Global.".into());
                    return true;
                }
                wizard.scope = if wizard.selected == 0 {
                    AgentProfileScope::Project
                } else {
                    AgentProfileScope::Global
                };
                wizard.enter_step(Step::Name);
            }
            Step::Name => {
                if wizard.validate_name() {
                    wizard.enter_step(Step::Description);
                }
            }
            Step::Description => {
                if wizard.draft.description.trim().is_empty()
                    || wizard.draft.description.chars().count() > 1000
                    || wizard.draft.description.contains(['\r', '\n'])
                {
                    wizard.error =
                        Some("Enter a single-line description, 1 to 1,000 characters.".into());
                } else {
                    wizard.enter_step(Step::Purpose);
                }
            }
            Step::Purpose => {
                wizard.enter_step(Step::Mode);
            }
            Step::Mode => {
                wizard.draft.mode = match wizard.selected {
                    1 => AgentMode::Primary,
                    2 => AgentMode::Subagent,
                    _ => AgentMode::All,
                };
                wizard.enter_step(Step::InstructionChoice);
            }
            Step::InstructionChoice => {
                if wizard.selected == 1 {
                    wizard.enter_step(Step::Instructions);
                } else if wizard.purpose.trim().is_empty() {
                    wizard.error = Some(
                        "Generation needs purpose/context. Esc to add it, or choose Write myself."
                            .into(),
                    );
                } else if !wizard.draft.prompt.is_empty() {
                    wizard.enter_step(Step::RegenerateConfirmation);
                } else {
                    self.start_agent_wizard_generation(wizard);
                }
            }
            Step::RegenerateConfirmation => {
                if wizard.selected == 0 {
                    wizard.enter_step(Step::Instructions);
                } else {
                    self.start_agent_wizard_generation(wizard);
                }
            }
            Step::Instructions => wizard.enter_step(Step::Model),
            Step::Model => {
                wizard.draft.model = if wizard.selected == 0 {
                    None
                } else {
                    wizard
                        .model_indices()
                        .into_iter()
                        .filter(|i| *i > 0)
                        .nth(wizard.selected - 1)
                        .map(|i| wizard.models[i].spec.clone())
                };
                if wizard
                    .draft
                    .effort
                    .as_ref()
                    .is_some_and(|effort| !wizard.efforts().contains(effort))
                {
                    wizard.draft.effort = None;
                }
                wizard.enter_step(Step::Effort);
            }
            Step::Effort => {
                wizard.draft.effort = wizard
                    .selected
                    .checked_sub(1)
                    .and_then(|i| wizard.efforts().get(i).cloned());
                wizard.enter_step(Step::Skills);
            }
            Step::Skills => {
                if wizard.draft.prompt.trim().is_empty() && wizard.draft.skills.is_empty() {
                    wizard.error = Some(
                        "Add instructions (Esc back) or select at least one skill with Space."
                            .into(),
                    );
                } else {
                    wizard.enter_step(Step::Review);
                }
            }
            Step::Review => match wizard.selected {
                0 | 1 => {
                    let activate = wizard.selected == 1;
                    if activate && !wizard.draft.mode.allows_primary() {
                        wizard.error =
                            Some("Workers-only profiles cannot activate in the main chat.".into());
                        return true;
                    }
                    if self.is_processing
                        || self.pending_turn
                        || self.agent_profile_switch_pending()
                        || self.remote_model_switch_in_flight
                        || self.pending_model_switch.is_some()
                        || self.pending_route_selection.is_some()
                    {
                        wizard.error = Some(
                            "A turn or profile/model change started. Wait before saving.".into(),
                        );
                        return true;
                    }
                    if !wizard.validate_name() {
                        return true;
                    }
                    match crate::agent_profile::create_profile(
                        wizard.scope,
                        wizard.working_dir.as_deref(),
                        &wizard.draft,
                    ) {
                        Ok(profile) => {
                            let path = profile.path.display().to_string();
                            let name = profile.name.clone();
                            if activate {
                                self.select_agent_profile(Some(&name));
                            }
                            self.open_agents_picker_with_error_reporting(false);
                            if let Some(picker) = self.inline_interactive_state.as_mut() {
                                if let Some(index) = picker.entries.iter().position(|entry| {
                                    entry.action
                                        == crate::tui::PickerAction::AgentProfile(Some(
                                            name.clone(),
                                        ))
                                }) {
                                    picker.selected = picker
                                        .filtered
                                        .iter()
                                        .position(|i| *i == index)
                                        .unwrap_or(0);
                                }
                            }
                            self.set_status_notice(if activate && self.is_remote {
                                format!("Saved {path}. Activation pending.")
                            } else if activate && self.active_agent_profile_name() != Some(&name) {
                                format!("Saved {path}. Activation failed. The file is preserved.")
                            } else {
                                format!("Saved {path}")
                            });
                            self.agent_wizard_saved_path = if activate && self.is_remote {
                                Some(path)
                            } else {
                                None
                            };
                            return false;
                        }
                        Err(error) => {
                            wizard.error =
                                Some(format!("Not saved: {error}. Your draft is intact."))
                        }
                    }
                }
                2 => wizard.enter_step(Step::Name),
                _ => {
                    wizard.return_step = Step::Review;
                    wizard.enter_step(Step::DiscardConfirmation);
                }
            },
            Step::DiscardConfirmation => {
                if wizard.selected == 1 {
                    wizard.cancel_generation();
                    return false;
                }
                wizard.enter_step(wizard.return_step);
            }
            Step::Generating => {}
        }
        true
    }
}

impl AgentWizard {
    fn start_clipboard(&mut self) {
        if self.clipboard.is_some() {
            return;
        }
        let (sender, receiver) = mpsc::channel();
        std::thread::spawn(move || {
            let _ = sender.send(input::read_clipboard_text());
        });
        self.clipboard = Some(ClipboardRead {
            revision: self.revision,
            receiver,
        });
    }

    fn poll_clipboard(&mut self) -> bool {
        let Some(pending) = &self.clipboard else {
            return false;
        };
        let Ok(text) = pending.receiver.try_recv() else {
            return false;
        };
        let valid = pending.revision == self.revision;
        self.clipboard = None;
        if valid {
            if let Some(text) = text {
                self.insert(&text);
            } else {
                self.error =
                    Some("Clipboard has no text. Paste or type instructions, not images.".into());
            }
        }
        true
    }

    fn edit_key(&mut self, code: KeyCode, modifiers: KeyModifiers, text: Option<&str>) {
        let control = modifiers.contains(KeyModifiers::CONTROL);
        match code {
            KeyCode::Char('z') if control => {
                if let Some((text, cursor)) = self.undo.pop() {
                    self.editor = text;
                    self.cursor = cursor;
                    self.revision = self.revision.wrapping_add(1);
                }
            }
            KeyCode::Home | KeyCode::Char('a') if code == KeyCode::Home || control => {
                self.cursor = 0
            }
            KeyCode::End | KeyCode::Char('e') if code == KeyCode::End || control => {
                self.cursor = self.editor.len()
            }
            KeyCode::Left => self.cursor = prev_char_boundary(&self.editor, self.cursor),
            KeyCode::Right => self.cursor = next_char_boundary(&self.editor, self.cursor),
            KeyCode::Backspace | KeyCode::Delete | KeyCode::Char('u' | 'k' | 'w')
                if matches!(code, KeyCode::Backspace | KeyCode::Delete) || control =>
            {
                self.remember_edit();
                let (start, end) = match code {
                    KeyCode::Char('u') => (0, self.cursor),
                    KeyCode::Char('k') => (self.cursor, self.editor.len()),
                    KeyCode::Char('w') => {
                        let prefix = &self.editor[..self.cursor];
                        let trimmed = prefix.trim_end();
                        let start = trimmed
                            .char_indices()
                            .rev()
                            .find(|(_, ch)| ch.is_whitespace())
                            .map(|(i, ch)| i + ch.len_utf8())
                            .unwrap_or(0);
                        (start, self.cursor)
                    }
                    KeyCode::Delete => (self.cursor, next_char_boundary(&self.editor, self.cursor)),
                    _ => (prev_char_boundary(&self.editor, self.cursor), self.cursor),
                };
                self.editor.drain(start..end);
                self.cursor = start;
                self.revision = self.revision.wrapping_add(1);
            }
            KeyCode::Up | KeyCode::Down if self.multiline() => {
                let prefix = &self.editor[..self.cursor];
                let start = prefix.rfind('\n').map(|i| i + 1).unwrap_or(0);
                let column = self.editor[start..self.cursor].chars().count();
                let target = if code == KeyCode::Up {
                    start
                        .checked_sub(1)
                        .map(|end| self.editor[..end].rfind('\n').map(|i| i + 1).unwrap_or(0))
                } else {
                    self.editor[self.cursor..]
                        .find('\n')
                        .map(|i| self.cursor + i + 1)
                };
                if let Some(start) = target {
                    let line = self.editor[start..].split('\n').next().unwrap_or("");
                    self.cursor = start
                        + line
                            .char_indices()
                            .nth(column)
                            .map(|(i, _)| i)
                            .unwrap_or(line.len());
                }
            }
            _ => {
                if let Some(text) = text {
                    self.insert(text);
                } else if let Some(text) = input::text_input_for_key(code, modifiers) {
                    self.insert(&text);
                }
            }
        }
    }
}
