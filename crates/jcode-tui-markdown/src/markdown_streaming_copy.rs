use super::*;

/// Return rendered copy targets for explicitly closed fenced code blocks.
/// Target coordinates are indices into `rendered_lines`.
pub fn extract_streaming_copy_targets(
    source: &str,
    rendered_lines: &[Line<'static>],
) -> Vec<RawCopyTarget> {
    let frames: Vec<_> = extract_copy_targets_from_rendered_lines(rendered_lines)
        .into_iter()
        .filter(|target| matches!(target.kind, CopyTargetKind::CodeBlock { .. }))
        .collect();
    if frames.is_empty() {
        return Vec::new();
    }
    let mut next_frame = 0;
    let mut result = Vec::new();
    let mut options = Options::empty();
    options.insert(
        Options::ENABLE_TABLES
            | Options::ENABLE_MATH
            | Options::ENABLE_STRIKETHROUGH
            | Options::ENABLE_TASKLISTS
            | Options::ENABLE_FOOTNOTES
            | Options::ENABLE_GFM
            | Options::ENABLE_DEFINITION_LIST
            | Options::ENABLE_SMART_PUNCTUATION,
    );
    let mut block: Option<(Option<(char, usize)>, String, usize, String)> = None;
    for (event, range) in Parser::new_ext(source, options).into_offset_iter() {
        match event {
            Event::Start(Tag::CodeBlock(kind)) => {
                let (fence, label) = match kind {
                    CodeBlockKind::Fenced(info) => {
                        let line = source[range.start..].lines().next().unwrap_or("");
                        (fence_run(line), info.to_string())
                    }
                    CodeBlockKind::Indented => (None, String::new()),
                };
                // Empty blocks have no Text event. Start after the opening
                // line so its delimiter cannot be mistaken for a closing one.
                let text_start = source[range.start..range.end]
                    .find('\n')
                    .map_or(range.end, |offset| range.start + offset + 1);
                block = Some((fence, label, text_start, String::new()));
            }
            Event::Text(text) => {
                if let Some((_, _, last_text_end, content)) = &mut block {
                    content.push_str(&text);
                    *last_text_end = range.end;
                }
            }
            Event::End(TagEnd::CodeBlock) => {
                let Some((fence, label, text_end, content)) = block.take() else {
                    continue;
                };
                let expected = content.lines().collect::<Vec<_>>().join("\n");
                // Other framed constructs can precede the fenced block. Match
                // source content as well as the complete rendered info label.
                let frame_label = if label.is_empty() { "code" } else { &label };
                let matching = |target: &RawCopyTarget| {
                    matches!(&target.kind, CopyTargetKind::CodeBlock { language }
                        if language.as_deref().unwrap_or("code") == frame_label)
                        && target.content == expected
                };
                // Diagrams may have no code frame at all. Do not advance
                // past later targets unless we actually find this source block.
                let Some(relative) = frames[next_frame..].iter().position(matching) else {
                    continue;
                };
                let index = next_frame + relative;
                next_frame = index + 1;
                let target = &frames[index];
                let Some((ch, count)) = fence else { continue };
                // End at EOF is synthesized by pulldown-cmark. Inspect only
                // source after the final code Text event, not the code body.
                let suffix = &source[text_end..range.end];
                let closed = suffix.lines().any(|line| {
                    // Container markers are outside Text events. The parser has
                    // already checked indentation and whether this is a closer.
                    let rest = line.trim_start_matches([' ', '\t', '>']);
                    let run = rest.chars().take_while(|c| *c == ch).count();
                    run >= count && rest[run..].trim().is_empty()
                });
                if closed {
                    result.push(target.clone());
                }
            }
            _ => {}
        }
    }
    result
}

fn fence_run(line: &str) -> Option<(char, usize)> {
    let bytes = line.as_bytes();
    let start = bytes.iter().position(|b| *b == b'`' || *b == b'~')?;
    let ch = bytes[start];
    let count = bytes[start..].iter().take_while(|b| **b == ch).count();
    (count >= 3).then_some((ch as char, count))
}
