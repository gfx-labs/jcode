use super::*;
use std::sync::Arc;

/// A half-open clickable range in terminal display cells, not UTF-8 bytes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LinkSpan {
    pub line: usize,
    pub start_col: usize,
    pub end_col: usize,
    pub target: Arc<str>,
}

#[derive(Clone, Debug, Default)]
pub struct RenderedMarkdown {
    pub lines: Vec<Line<'static>>,
    pub links: Vec<LinkSpan>,
}

impl From<Vec<Line<'static>>> for RenderedMarkdown {
    fn from(lines: Vec<Line<'static>>) -> Self {
        Self {
            lines,
            links: Vec::new(),
        }
    }
}

impl RenderedMarkdown {
    pub fn append(&mut self, other: Self) {
        self.links.extend(other.links.into_iter().map(|mut link| {
            link.line += self.lines.len();
            link
        }));
        self.lines.extend(other.lines);
    }

    pub fn wrapped(self, width: usize) -> Self {
        let mut rendered = Self::default();
        let mut by_line: HashMap<usize, Vec<LinkSpan>> = HashMap::new();
        for link in self.links {
            by_line.entry(link.line).or_default().push(link);
        }
        for (index, line) in self.lines.into_iter().enumerate() {
            rendered.append(wrap_line_with_links(
                line,
                width,
                by_line.get(&index).map(Vec::as_slice).unwrap_or(&[]),
            ));
        }
        rendered
    }
}

pub(crate) struct TableLinkSpan {
    pub row: usize,
    pub column: usize,
    pub start_col: usize,
    pub end_col: usize,
    pub target: Arc<str>,
}

pub(crate) fn finish_link(
    links: &mut Vec<LinkSpan>,
    lines: &[Line<'static>],
    current_spans: &[Span<'static>],
    start: (usize, usize),
    target: String,
) {
    if target.is_empty() {
        return;
    }
    let target: Arc<str> = target.into();
    for line in start.0..=lines.len() {
        let spans = if line == lines.len() {
            current_spans
        } else {
            &lines[line].spans
        };
        let start_col = if line == start.0 { start.1 } else { 0 };
        let end_col = spans.iter().map(Span::width).sum();
        if start_col < end_col {
            links.push(LinkSpan {
                line,
                start_col,
                end_col,
                target: target.clone(),
            });
        }
    }
}

pub(crate) fn adjust_link_padding(
    links: &mut [LinkSpan],
    line: usize,
    before: usize,
    after: usize,
) {
    for link in links.iter_mut().filter(|link| link.line == line) {
        link.start_col = link.start_col.saturating_add(after).saturating_sub(before);
        link.end_col = link.end_col.saturating_add(after).saturating_sub(before);
    }
}

/// Keep semantic ranges attached when centering structured blocks inserts padding.
pub fn recenter_markdown(rendered: &mut RenderedMarkdown, width: usize) {
    let widths: Vec<_> = rendered.lines.iter().map(Line::width).collect();
    recenter_structured_blocks_for_display(&mut rendered.lines, width);
    for (line, before) in widths.into_iter().enumerate() {
        adjust_link_padding(
            &mut rendered.links,
            line,
            before,
            rendered.lines[line].width(),
        );
    }
}

/// Wrap one logical line and carry its links through hanging gutters and balanced wrapping.
pub fn wrap_line_with_links(
    line: Line<'static>,
    width: usize,
    links: &[LinkSpan],
) -> RenderedMarkdown {
    if links.is_empty() {
        return wrap_line(line, width).into();
    }
    let source = line_plain_text(&line);
    let prefix_width = repeated_gutter_prefix(&line)
        .map(|(_, width)| width)
        .filter(|&prefix| prefix < width)
        .unwrap_or(0);
    let lines = wrap_line(line, width);
    let mut mapped = Vec::new();
    let mut source_byte = 0;
    for (row, line) in lines.iter().enumerate() {
        let plain = line_plain_text(line);
        let prefix = if row > 0 { prefix_width } else { 0 };
        let body_byte = byte_at_column(&plain, prefix);
        let body = &plain[body_byte..];
        // Balanced wrapping drops the inter-word space at the break.
        while !source[source_byte..].starts_with(body) {
            let Some(ch) = source[source_byte..]
                .chars()
                .next()
                .filter(|ch| ch.is_whitespace())
            else {
                break;
            };
            source_byte += ch.len_utf8();
        }
        if !source[source_byte..].starts_with(body) {
            continue;
        }
        let start = UnicodeWidthStr::width(&source[..source_byte]);
        let end = start + UnicodeWidthStr::width(body);
        for link in links {
            let left = start.max(link.start_col);
            let right = end.min(link.end_col);
            if left < right {
                mapped.push(LinkSpan {
                    line: row,
                    start_col: prefix + left - start,
                    end_col: prefix + right - start,
                    target: link.target.clone(),
                });
            }
        }
        source_byte += body.len();
    }
    RenderedMarkdown {
        lines,
        links: mapped,
    }
}

fn byte_at_column(text: &str, column: usize) -> usize {
    let mut col = 0;
    for (byte, ch) in text.char_indices() {
        if col >= column {
            return byte;
        }
        col += unicode_width::UnicodeWidthChar::width(ch).unwrap_or(0);
    }
    text.len()
}
