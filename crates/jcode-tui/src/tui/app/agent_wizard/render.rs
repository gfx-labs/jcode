use super::*;
use ratatui::{
    Frame,
    layout::Rect,
    style::{Color, Modifier, Style},
    text::Line,
    widgets::{Block, BorderType, Borders, Clear, Paragraph},
};
use unicode_width::UnicodeWidthChar;

impl AgentWizard {
    pub(crate) fn capture_debug_frame(&self, area: Rect, elapsed: std::time::Duration) {
        use crate::tui::visual_debug::{self, FrameCaptureBuilder, RenderTimingCapture};
        if !visual_debug::is_enabled() {
            return;
        }
        let mut capture = FrameCaptureBuilder::new(area.width, area.height);
        capture.layout.input_area = Some(area.into());
        capture.state.input_len = self.editor.len();
        capture.state.cursor_pos = self.cursor;
        capture.state.scroll_offset = self.scroll;
        capture.state.status = format!(
            "{}:choice={}:revision={}",
            self.step_title(),
            self.selected,
            self.revision
        );
        capture.rendered_text.status_line = format!("Create new agent · {}", self.step_title());
        capture.render_order.push("agent_wizard".into());
        let milliseconds = elapsed.as_secs_f32() * 1000.0;
        capture.render_timing = Some(RenderTimingCapture {
            prepare_ms: 0.0,
            draw_ms: milliseconds,
            total_ms: milliseconds,
            messages_ms: None,
            widgets_ms: None,
        });
        visual_debug::record_frame(capture.build());
    }

    pub(crate) fn render(&self, frame: &mut Frame<'_>) {
        let screen = frame.area();
        if screen.width == 0 || screen.height == 0 {
            return;
        }
        frame.render_widget(Clear, screen);
        let width = screen.width.min(104);
        let area = Rect::new(
            screen.x + screen.width.saturating_sub(width) / 2,
            screen.y,
            width,
            screen.height,
        );
        let title = format!(" Create new agent · {} ", self.step_title());
        let block = Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .title(title)
            .border_style(Style::default().fg(Color::DarkGray));
        let inner = block.inner(area);
        frame.render_widget(block, area);
        if inner.width == 0 || inner.height == 0 {
            return;
        }
        let width = inner.width as usize;
        let hint = if self.step == Step::Generating {
            "Esc cancels generation. No file or chat turn is created."
        } else if self.step == Step::Skills {
            "Type to filter · Space toggles · Enter next · Esc back · Ctrl+C cancel"
        } else if self.multiline() {
            "Enter next · Shift/Alt+Enter newline · Esc back · PgUp/PgDn scroll · Ctrl+C cancel"
        } else {
            "Enter confirms · arrows select · Esc back · PgUp/PgDn scroll · Ctrl+C cancel"
        };
        let mut footer = Vec::new();
        if let Some(error) = &self.error {
            footer.extend(
                wrap(error, width)
                    .into_iter()
                    .map(|line| Line::styled(line, Style::default().fg(Color::Red))),
            );
        }
        if let Some(warning) = &self.warning {
            footer.extend(
                wrap(warning, width)
                    .into_iter()
                    .map(|line| Line::styled(line, Style::default().fg(Color::Yellow))),
            );
        }
        footer.extend(
            wrap(hint, width)
                .into_iter()
                .map(|line| Line::styled(line, Style::default().fg(Color::DarkGray))),
        );
        let footer_height = footer.len().min((inner.height as usize / 2).max(1));
        let body_height = inner.height as usize - footer_height;
        let mut body = Vec::new();
        for line in wrap(self.explanation(), width)
            .into_iter()
            .take(body_height.saturating_sub(2).min(3))
        {
            body.push(Line::from(line));
        }
        let choices = self.choices();
        if !choices.is_empty() {
            let reserved_detail = usize::from(self.step == Step::Review || self.text_step());
            let capacity = body_height
                .saturating_sub(body.len() + reserved_detail)
                .max(1)
                .min(8);
            let start = self.selected.saturating_sub(capacity.saturating_sub(1));
            for (i, choice) in choices.iter().enumerate().skip(start).take(capacity) {
                let marker = if i == self.selected { ">" } else { " " };
                let style = if i == self.selected {
                    Style::default()
                        .fg(Color::Cyan)
                        .add_modifier(Modifier::BOLD)
                } else {
                    Style::default()
                };
                // Long labels stay bounded without letting one option hide all remaining controls.
                let line = wrap(&format!("{marker} {choice}"), width)
                    .into_iter()
                    .next()
                    .unwrap_or_default();
                body.push(Line::styled(line, style));
            }
            if choices.len() > capacity && body.len() + reserved_detail < body_height {
                body.push(Line::styled(
                    format!("{} / {}", self.selected + 1, choices.len()),
                    Style::default().fg(Color::DarkGray),
                ));
            }
        }
        let detail = if self.step == Step::Review {
            let destination = crate::agent_profile::profile_destination(
                self.scope,
                self.working_dir.as_deref(),
                &self.draft.name,
            )
            .map(|path| path.display().to_string())
            .unwrap_or_else(|error| error.to_string());
            format!(
                "Name: {}\nDestination: {}\nDescription: {}\nUsage: {}\nModel: {}\nEffort: {}\nSkills: {}\n\n{}",
                self.draft.name,
                destination,
                self.draft.description,
                generation::wizard_mode_name(self.draft.mode),
                self.draft.model.as_deref().unwrap_or("Inherit"),
                self.draft.effort.as_deref().unwrap_or("No override"),
                if self.draft.skills.is_empty() {
                    "None".into()
                } else {
                    self.draft.skills.join(", ")
                },
                self.draft.prompt
            )
        } else if self.text_step() {
            let mut text = self.editor.clone();
            text.insert(self.cursor.min(text.len()), '▏');
            if matches!(self.step, Step::Model | Step::Skills) {
                format!("Filter: {text}")
            } else {
                text
            }
        } else if matches!(self.step, Step::InstructionChoice | Step::Generating) {
            format!(
                "Generator: {}\nUses your configured model connection, not the future profile model.\nExisting instructions: {} bytes",
                self.generator_identity,
                self.draft.prompt.len()
            )
        } else if self.step == Step::Location {
            let project = self
                .working_dir
                .as_ref()
                .map(|path| path.join(".jcode/agents").display().to_string())
                .unwrap_or_else(|| "unavailable".into());
            let global = crate::agent_profile::profile_destination(
                AgentProfileScope::Global,
                None,
                "agent-name",
            )
            .map(|path| path.parent().unwrap_or(&path).display().to_string())
            .unwrap_or_else(|error| error.to_string());
            format!("Project: {project}\nGlobal: {global}")
        } else {
            String::new()
        };
        let lines = wrap(&detail, width);
        let remaining = body_height.saturating_sub(body.len());
        self.detail_rows.set(remaining);
        let cursor_line = if self.text_step() && !matches!(self.step, Step::Model | Step::Skills) {
            wrap(&self.editor[..self.cursor], width)
                .len()
                .saturating_sub(1)
        } else {
            0
        };
        let automatic = cursor_line.saturating_sub(remaining.saturating_sub(1));
        let offset = if self.scroll > 0 {
            self.scroll
        } else {
            automatic
        }
        .min(lines.len().saturating_sub(remaining));
        for line in lines.into_iter().skip(offset).take(remaining) {
            body.push(Line::from(line));
        }
        frame.render_widget(
            Paragraph::new(body),
            Rect::new(inner.x, inner.y, inner.width, body_height as u16),
        );
        frame.render_widget(
            Paragraph::new(footer),
            Rect::new(
                inner.x,
                inner.y + body_height as u16,
                inner.width,
                footer_height as u16,
            ),
        );
    }

    fn step_title(&self) -> &'static str {
        match self.step {
            Step::Location => "1/11 Location",
            Step::Name => "2/11 Name",
            Step::Description => "3/11 Description",
            Step::Purpose => "4/11 Purpose",
            Step::Mode => "5/11 Usage",
            Step::InstructionChoice => "6/11 Instructions",
            Step::Instructions => "7/11 Edit instructions",
            Step::Model => "8/11 Model",
            Step::Effort => "9/11 Effort",
            Step::Skills => "10/11 Skills",
            Step::Review => "11/11 Review",
            Step::DiscardConfirmation => "Discard?",
            Step::RegenerateConfirmation => "Replace instructions?",
            Step::Generating => "Generating draft",
        }
    }

    fn explanation(&self) -> &'static str {
        match self.step {
            Step::Location => "Choose where this editable Markdown profile will be saved.",
            Step::Name => {
                "Name: ASCII letters, digits, hyphens and underscores. Maximum 64 characters."
            }
            Step::Description => {
                "Description: one line, up to 1,000 characters. Shown in the agent picker."
            }
            Step::Purpose => {
                "Purpose/context: up to 4 KiB. Required for generation, optional for manual entry. Not saved as metadata."
            }
            Step::Mode => {
                "Choose where this preset can be used. Modes are not permission sandboxes."
            }
            Step::InstructionChoice => {
                "Generate an isolated draft, or write instructions yourself. Nothing is saved yet."
            }
            Step::Instructions => {
                "Edit instructions, up to 64 KiB. Empty is allowed when skills are selected later."
            }
            Step::Model => {
                "Future profile model only. Selecting here does not change the current chat."
            }
            Step::Effort => {
                "No override omits effort. Inherit uses current-route choices. Another route may reject this effort on activation."
            }
            Step::Skills => {
                "Optional installed skills. Only their names are saved, not their contents."
            }
            Step::Review => {
                "Review before saving. Profiles are instruction/model presets, not permission sandboxes."
            }
            Step::DiscardConfirmation => {
                "Discard this unsaved draft? Your original composer is unchanged."
            }
            Step::RegenerateConfirmation => {
                "Generating again replaces the existing instructions only after a successful response."
            }
            Step::Generating => {
                "Generating instructions privately. The conversation, tools and files are not sent as context."
            }
        }
    }
}

fn wrap(text: &str, width: usize) -> Vec<String> {
    let width = width.max(1);
    let mut rows = Vec::new();
    for line in text.split('\n') {
        let mut row = String::new();
        let mut used = 0;
        for ch in line.chars() {
            let size = ch.width().unwrap_or(0);
            if used + size > width && !row.is_empty() {
                rows.push(std::mem::take(&mut row));
                used = 0;
            }
            if size <= width {
                row.push(ch);
                used += size;
            }
        }
        rows.push(row);
    }
    rows
}
