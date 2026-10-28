//! Assistant plan-card segmentation and rendering.
use super::*;

pub(super) fn split_plan_segments(content: &str) -> Option<Vec<AssistantSegment>> {
    if !content.contains("```plan") {
        return None;
    }

    let mut segments: Vec<AssistantSegment> = Vec::new();
    let mut current = String::new();
    let mut plan_body: Option<String> = None;
    let mut plan_nested_fence = false;
    let mut in_other_fence = false;
    let mut saw_plan = false;

    for line in content.split('\n') {
        let trimmed = line.trim_start();
        if let Some(body) = plan_body.as_mut() {
            let is_fence_line = trimmed.starts_with("```");
            let is_bare_fence = is_fence_line && trimmed.trim_end() == "```";
            if is_bare_fence && !plan_nested_fence {
                let body = std::mem::take(body);
                plan_body = None;
                segments.push(AssistantSegment::Plan(body));
            } else {
                if is_fence_line {
                    // A nested fenced block inside the plan (e.g. ```bash ...
                    // ```). Its closing bare fence must not end the plan.
                    plan_nested_fence = !plan_nested_fence;
                }
                if !body.is_empty() {
                    body.push('\n');
                }
                body.push_str(line);
            }
            continue;
        }

        if !in_other_fence
            && trimmed
                .strip_prefix("```plan")
                .is_some_and(|rest| rest.trim().is_empty())
        {
            saw_plan = true;
            plan_nested_fence = false;
            if !current.trim().is_empty() {
                segments.push(AssistantSegment::Markdown(std::mem::take(&mut current)));
            } else {
                current.clear();
            }
            plan_body = Some(String::new());
            continue;
        }

        if trimmed.starts_with("```") {
            in_other_fence = !in_other_fence;
        }
        if !current.is_empty() {
            current.push('\n');
        }
        current.push_str(line);
    }

    // Unterminated plan fence (e.g. mid-stream): render what we have as a card.
    if let Some(body) = plan_body.take() {
        segments.push(AssistantSegment::Plan(body));
    }
    if !current.trim().is_empty() {
        segments.push(AssistantSegment::Markdown(current));
    }

    saw_plan.then_some(segments)
}

pub(super) fn render_assistant_segments_with_links(
    segments: &[AssistantSegment],
    width: u16,
    wrap_width: usize,
) -> markdown::RenderedMarkdown {
    let mut rendered = markdown::RenderedMarkdown::default();
    for segment in segments {
        match segment {
            AssistantSegment::Markdown(text) => {
                if !rendered.lines.is_empty() {
                    rendered.lines.push(Line::from(""));
                }
                rendered.append(markdown::render_markdown_with_links(text, Some(wrap_width)));
            }
            AssistantSegment::Plan(body) => {
                if !rendered.lines.is_empty() {
                    rendered.lines.push(Line::from(""));
                }
                rendered.append(render_plan_card_with_links(body, width));
            }
        }
    }
    rendered
}

/// Render the inner markdown of a ```plan block as a bordered plan card.
pub(super) fn render_plan_card_with_links(body: &str, width: u16) -> markdown::RenderedMarkdown {
    let border_style = Style::default().fg(rgb(158, 135, 255));
    let max_box_width = (width.saturating_sub(4) as usize).clamp(10, 100);
    let inner_width = max_box_width.saturating_sub(4).max(6);

    let title = plan_card_title(body);
    let body_without_title = plan_card_body_without_title(body, &title);

    // `render_markdown_with_width` sizes block elements (code, tables, rules)
    // but does not hard-wrap paragraph text; the normal message path wraps
    // later in the pipeline. The card boxes its content immediately and
    // `render_rounded_box` truncates over-long lines, so wrap here to avoid
    // cutting plan text off at the border.
    let rendered = markdown::render_markdown_with_links(&body_without_title, Some(inner_width))
        .wrapped(inner_width);
    let mut links = rendered.links;
    let mut content = rendered.lines;
    // Trim leading/trailing blank rows inside the card.
    while content.first().is_some_and(|line| line.width() == 0) {
        content.remove(0);
        links.retain_mut(|link| {
            if link.line == 0 {
                return false;
            }
            link.line -= 1;
            true
        });
    }
    while content.last().is_some_and(|line| line.width() == 0) {
        content.pop();
    }
    if content.is_empty() {
        content.push(Line::from(Span::styled(
            "(empty plan)",
            Style::default().fg(dim_color()),
        )));
    }

    let lines = render_rounded_box(&title, content, max_box_width, border_style);
    for link in &mut links {
        link.line += 1;
        link.start_col += 2;
        link.end_col += 2;
    }
    links.retain(|link| {
        link.line < lines.len() && link.end_col <= lines[link.line].width().saturating_sub(2)
    });
    markdown::RenderedMarkdown { lines, links }
}

/// Title for the plan card: the first markdown heading in the body, else "Plan".
pub(super) fn plan_card_title(body: &str) -> String {
    for line in body.lines() {
        let trimmed = line.trim();
        if let Some(heading) = trimmed
            .strip_prefix("# ")
            .or_else(|| trimmed.strip_prefix("## "))
            .or_else(|| trimmed.strip_prefix("### "))
        {
            let heading = heading.trim();
            if !heading.is_empty() {
                return format!("⛭ {}", heading);
            }
        }
        if !trimmed.is_empty() {
            break;
        }
    }
    "⛭ Plan".to_string()
}

/// Remove the first heading line when it was promoted to the card title.
pub(super) fn plan_card_body_without_title(body: &str, title: &str) -> String {
    if title == "⛭ Plan" {
        return body.to_string();
    }
    let mut removed = false;
    body.lines()
        .filter(|line| {
            if removed {
                return true;
            }
            let trimmed = line.trim();
            if trimmed.is_empty() {
                return true;
            }
            if trimmed.starts_with('#') {
                removed = true;
                return false;
            }
            removed = true;
            true
        })
        .collect::<Vec<_>>()
        .join("\n")
}
