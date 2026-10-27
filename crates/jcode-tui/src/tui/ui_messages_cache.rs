use super::*;

pub(super) use jcode_tui_messages::{centered_wrap_width, left_pad_lines_for_centered_mode};

pub(crate) fn get_cached_message_lines<F>(
    msg: &DisplayMessage,
    width: u16,
    diff_mode: crate::config::DiffDisplayMode,
    render: F,
) -> Vec<Line<'static>>
where
    F: FnOnce(&DisplayMessage, u16, crate::config::DiffDisplayMode) -> Vec<Line<'static>>,
{
    get_cached_message_render(msg, width, diff_mode, |msg, width, diff_mode| {
        render(msg, width, diff_mode).into()
    })
    .lines
}

pub(crate) fn get_cached_message_render<F>(
    msg: &DisplayMessage,
    width: u16,
    diff_mode: crate::config::DiffDisplayMode,
    render: F,
) -> markdown::RenderedMarkdown
where
    F: FnOnce(&DisplayMessage, u16, crate::config::DiffDisplayMode) -> markdown::RenderedMarkdown,
{
    // Relative call ages change even when the notification content does not.
    if msg.role == "background_task" && msg.content.contains("_Called at:_ `") {
        return render(msg, width, diff_mode);
    }
    jcode_tui_messages::get_cached_message_render(
        msg,
        width,
        diff_mode,
        jcode_tui_messages::MessageCacheContext {
            diagram_mode: crate::config::config().display.diagram_mode,
            centered: markdown::center_code_blocks(),
            // Message lines contain Mermaid placeholder rows. Size clicks must
            // invalidate this cache just like a completed deferred render does.
            mermaid_epoch: crate::tui::mermaid::deferred_render_epoch()
                .wrapping_add(crate::tui::mermaid::mermaid_inline_expand_epoch()),
            mermaid_aspect_bucket: crate::tui::mermaid::current_preferred_aspect_ratio_bucket(),
            show_agentgrep_output: crate::config::config().display.show_agentgrep_output,
            show_bash_output: crate::config::config().display.show_bash_output,
            tool_call_details: crate::config::config().display.tool_call_details,
            compact_transcript: crate::config::config().display.compact_transcript,
            reasoning_display: crate::config::config().display.reasoning_display(),
            latex_rendering: crate::config::config().display.latex_rendering,
            compact_links: crate::config::config().display.compact_links,
        },
        render,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mermaid_expand_transition_invalidates_cached_message_lines() {
        let hash = 0xf4ce_a123_u64;
        let msg = DisplayMessage::assistant(format!(
            "```mermaid\nflowchart LR\nA[cache-{hash}] --> B\n```"
        ));

        crate::tui::mermaid::set_mermaid_inline_expand_level(hash, 0);
        let first =
            get_cached_message_lines(&msg, 80, crate::config::DiffDisplayMode::Off, |_, _, _| {
                vec![Line::from("fit")]
            });
        assert_eq!(first.len(), 1);

        crate::tui::mermaid::set_mermaid_inline_expand_level(hash, 1);
        let second =
            get_cached_message_lines(&msg, 80, crate::config::DiffDisplayMode::Off, |_, _, _| {
                vec![Line::from("large-1"), Line::from("large-2")]
            });
        crate::tui::mermaid::set_mermaid_inline_expand_level(hash, 0);

        assert_eq!(
            second.len(),
            2,
            "the message cache must rerender after a Mermaid size transition"
        );
    }
}
