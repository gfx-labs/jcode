//! Reasoning-line markdown formatting.
//!
//! Pure string helpers shared by the server/streaming path and the TUI renderer
//! so the wrapping/escaping rules stay in lockstep with the renderer that
//! consumes them. These live in `jcode-render-core` (a backend-neutral, pure
//! crate) rather than in `jcode-tui-markdown` so the foundation/streaming layer
//! can format reasoning lines without depending on any `jcode-tui-*` crate.

/// Invisible separator placed just inside both ends of an emphasis run so the
/// flanking `*` are always adjacent to non-whitespace (see
/// [`reasoning_line_markup`]).
pub const REASONING_SENTINEL: &str = "\u{2063}";

/// Escape the characters that would otherwise be interpreted as inline markdown
/// inside a reasoning line, so the body renders literally inside the dim/italic
/// emphasis run.
fn escape_reasoning_inline_markdown(line: &str) -> String {
    let mut out = String::with_capacity(line.len() + 8);
    for ch in line.chars() {
        match ch {
            '\\' | '*' | '_' | '`' | '[' | ']' | '<' | '>' | '&' | '~' | '|' | '$' => {
                out.push('\\');
                out.push(ch);
            }
            _ => out.push(ch),
        }
    }
    out
}

/// Wrap a completed reasoning line as dim+italic markdown.
///
/// Empty lines become a bare newline (no empty emphasis run). The result always
/// ends in a CommonMark hard break (`"  \n"`).
///
/// The trailing two spaces are a CommonMark *hard break*: without them,
/// consecutive reasoning lines (each terminated by a single `\n`) collapse into
/// one paragraph where the line breaks render as spaces, so multi-line thinking
/// shows up as a single run-on line. The hard break keeps each reasoning line on
/// its own visual row, matching the model's line structure.
///
/// The sentinel must wrap both ends because CommonMark's emphasis flanking rules
/// require the opening `*` to not be followed by whitespace and the closing `*`
/// to not be preceded by whitespace. A reasoning line that starts or ends with
/// whitespace (or is whitespace-only) would otherwise leave the asterisks as
/// literal text and break the dim/italic styling. The zero-width sentinels
/// guarantee both asterisks are flanked by non-whitespace regardless of the body.
pub fn reasoning_line_markup(line: &str) -> String {
    if line.is_empty() {
        "\n".to_string()
    } else {
        format!(
            "*{0}{1}{0}*  \n",
            REASONING_SENTINEL,
            escape_reasoning_inline_markdown(line)
        )
    }
}

/// Wrap the in-progress (not yet newline-terminated) reasoning line as dim+italic
/// markdown, identical to [`reasoning_line_markup`] but *without* the trailing
/// newline so it renders as the live tail of the streaming buffer. Callers
/// truncate and re-emit this tail on each streamed delta so reasoning trickles in
/// token-by-token instead of one whole line at a time. An empty line yields an
/// empty string (nothing to render yet).
pub fn reasoning_partial_markup(line: &str) -> String {
    if line.is_empty() {
        String::new()
    } else {
        format!(
            "*{0}{1}{0}*",
            REASONING_SENTINEL,
            escape_reasoning_inline_markdown(line)
        )
    }
}

/// One-line collapsed reasoning summary markup (e.g. `▸ thought (3 lines)`),
/// styled dim+italic like the live reasoning lines. Used to fold a persisted
/// reasoning block down to a single trace line when the transcript is
/// re-rendered from history in `current` reasoning-display mode (so reloaded /
/// resumed sessions match the live collapse instead of replaying every line).
///
/// Lives here (a backend-neutral, pure crate) rather than in `jcode-tui-markdown`
/// so the foundation/streaming layer can format the summary without depending on
/// any `jcode-tui-*` crate. Re-exported from `jcode-tui-markdown` for the
/// existing `jcode_tui_markdown::reasoning_summary_line_markup` path.
pub fn reasoning_summary_line_markup(line_count: usize) -> String {
    let label = match line_count {
        0 | 1 => "▸ thought".to_string(),
        n => format!("▸ thought ({} lines)", n),
    };
    reasoning_line_markup(&label)
}

/// Undo [`reasoning_line_markup`] for one line: strip the emphasis wrapper,
/// the sentinels, the trailing hard break, and the inline-markdown escapes so
/// the original reasoning text comes back out. Returns `None` when the line is
/// not a reasoning line (no sentinel).
pub fn unescape_reasoning_line(line: &str) -> Option<String> {
    if !line.contains(REASONING_SENTINEL) {
        return None;
    }
    let trimmed = line.trim_end();
    let trimmed = trimmed.strip_prefix('*').unwrap_or(trimmed);
    let trimmed = trimmed.strip_suffix('*').unwrap_or(trimmed);
    let stripped = trimmed.replace(REASONING_SENTINEL, "");
    let mut out = String::with_capacity(stripped.len());
    let mut chars = stripped.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch == '\\'
            && let Some(&next) = chars.peek()
            && matches!(
                next,
                '\\' | '*' | '_' | '`' | '[' | ']' | '<' | '>' | '&' | '~' | '|' | '$'
            )
        {
            out.push(next);
            chars.next();
            continue;
        }
        out.push(ch);
    }
    Some(out)
}

/// A contiguous run of reasoning lines inside a committed message body.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReasoningBlock {
    /// Byte range of the block within the source content (start of the first
    /// reasoning line through the end of the last one, including its newline).
    pub start: usize,
    pub end: usize,
    /// Reasoning text with markup removed, one entry per line.
    pub lines: Vec<String>,
}

impl ReasoningBlock {
    /// Plain reasoning text, lines joined with `\n`.
    pub fn text(&self) -> String {
        self.lines.join("\n")
    }

    /// Approximate word count of the reasoning text.
    pub fn word_count(&self) -> usize {
        self.lines
            .iter()
            .map(|line| line.split_whitespace().count())
            .sum()
    }
}

/// Split committed content into its reasoning blocks. Reasoning lines are
/// those carrying [`REASONING_SENTINEL`]; consecutive ones (blank lines in
/// between allowed) form one block. Returns an empty vec for content with no
/// reasoning.
pub fn extract_reasoning_blocks(content: &str) -> Vec<ReasoningBlock> {
    let mut blocks: Vec<ReasoningBlock> = Vec::new();
    let mut current: Option<ReasoningBlock> = None;
    let mut offset = 0usize;
    for raw in content.split_inclusive('\n') {
        let start = offset;
        offset += raw.len();
        let line = raw.trim_end_matches('\n').trim_end_matches('\r');
        if let Some(text) = unescape_reasoning_line(line) {
            match current.as_mut() {
                Some(block) => {
                    block.end = offset;
                    block.lines.push(text);
                }
                None => {
                    current = Some(ReasoningBlock {
                        start,
                        end: offset,
                        lines: vec![text],
                    });
                }
            }
        } else if !line.trim().is_empty()
            && let Some(block) = current.take()
        {
            // Blank lines inside a run do not end it; any other text does.
            blocks.push(block);
        }
    }
    if let Some(block) = current.take() {
        blocks.push(block);
    }
    blocks
}

/// Whether the content consists only of reasoning lines and whitespace.
pub fn content_is_reasoning_only(content: &str) -> bool {
    let mut saw_reasoning = false;
    for line in content.lines() {
        if line.contains(REASONING_SENTINEL) {
            saw_reasoning = true;
        } else if !line.trim().is_empty() {
            return false;
        }
    }
    saw_reasoning
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unescape_round_trips_reasoning_line_markup() {
        let original = "Use `cargo test` on *this* [thing] <tag> & 100% | $x";
        let markup = reasoning_line_markup(original);
        let line = markup.trim_end_matches('\n');
        assert_eq!(unescape_reasoning_line(line).as_deref(), Some(original));
        assert_eq!(unescape_reasoning_line("plain text"), None);
    }

    #[test]
    fn extract_reasoning_blocks_splits_on_answer_text_and_spans_blanks() {
        let mut content = String::new();
        content.push_str(&reasoning_line_markup("first"));
        content.push('\n');
        content.push_str(&reasoning_line_markup("second"));
        content.push_str("\nThe answer.\n");
        content.push_str(&reasoning_line_markup("third"));

        let blocks = extract_reasoning_blocks(&content);
        assert_eq!(blocks.len(), 2, "{blocks:?}");
        assert_eq!(blocks[0].lines, vec!["first", "second"]);
        assert_eq!(blocks[1].lines, vec!["third"]);
        assert_eq!(blocks[0].word_count(), 2);

        // Splicing the blocks out leaves only the answer.
        let mut answer = String::new();
        let mut cursor = 0;
        for block in &blocks {
            answer.push_str(&content[cursor..block.start]);
            cursor = block.end;
        }
        answer.push_str(&content[cursor..]);
        assert_eq!(answer.trim(), "The answer.");
    }

    #[test]
    fn content_is_reasoning_only_detects_pure_traces() {
        let pure = format!(
            "{}{}",
            reasoning_line_markup("a"),
            reasoning_line_markup("b")
        );
        assert!(content_is_reasoning_only(&pure));
        assert!(!content_is_reasoning_only(&format!("{pure}answer")));
        assert!(!content_is_reasoning_only("no reasoning here"));
        assert!(!content_is_reasoning_only(""));
    }
}
