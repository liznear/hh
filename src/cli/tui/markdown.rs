use std::sync::OnceLock;

use ratatui::{
    prelude::Stylize,
    style::{Color, Style},
    text::{Line, Span},
};
use syntect::{
    easy::HighlightLines,
    highlighting::{Theme, ThemeSet},
    parsing::{SyntaxReference, SyntaxSet},
};
use syntect_tui::translate_style;

pub fn markdown_to_lines_with_indent(
    markdown: &str,
    width: usize,
    indent: &str,
) -> Vec<Line<'static>> {
    let mut rendered = Vec::new();
    let mut in_code_block = false;
    let mut code_fence = CodeFence::default();
    let mut code_lines: Vec<String> = Vec::new();

    for line in markdown.lines() {
        if let Some(fence) = parse_code_fence(line) {
            if in_code_block {
                rendered.extend(render_code_block(&code_lines, &code_fence, width, indent));
                in_code_block = false;
                code_fence = CodeFence::default();
                code_lines.clear();
            } else {
                in_code_block = true;
                code_fence = fence;
            }
            continue;
        }

        if in_code_block {
            code_lines.push(line.to_string());
            continue;
        }

        rendered.extend(render_text_line(line, width, indent));
    }

    if in_code_block {
        rendered.extend(render_code_block(&code_lines, &code_fence, width, indent));
    }

    rendered
}

#[derive(Default, Clone)]
struct CodeFence {
    language: String,
    location_hint: Option<String>,
}

fn parse_code_fence(line: &str) -> Option<CodeFence> {
    let trimmed = line.trim_start();
    if !trimmed.starts_with("```") {
        return None;
    }

    let info = trimmed.trim_start_matches("```").trim();
    if info.is_empty() {
        return Some(CodeFence::default());
    }

    let mut parts = info.split_whitespace();
    let language = parts.next().unwrap_or("").to_string();
    let metadata = parts.collect::<Vec<_>>().join(" ");
    let location_hint = parse_location_hint(&metadata);

    Some(CodeFence {
        language,
        location_hint,
    })
}

fn render_text_line(line: &str, width: usize, indent: &str) -> Vec<Line<'static>> {
    let spans = parse_inline_markdown_spans(line);
    let wrapped = wrap_spans(&spans, width.saturating_sub(indent.chars().count()));

    wrapped
        .into_iter()
        .map(|wrapped_line| {
            let mut indented = vec![Span::raw(indent.to_string())];
            indented.extend(wrapped_line);
            Line::from(indented)
        })
        .collect()
}

const CODE_BLOCK_BG: Color = Color::Rgb(236, 240, 248);
const CODE_BLOCK_HEADER_FG: Color = Color::Rgb(86, 96, 113);
const CODE_BLOCK_SEPARATOR_FG: Color = Color::Rgb(184, 193, 207);
const CODE_BLOCK_PADDING_X: usize = 2;

fn render_code_block(
    code_lines: &[String],
    fence: &CodeFence,
    width: usize,
    indent: &str,
) -> Vec<Line<'static>> {
    let styled_code_lines =
        highlight_code_block(code_lines, &fence.language).unwrap_or_else(|| {
            code_lines
                .iter()
                .map(|line| vec![Span::raw(line.clone())])
                .collect()
        });

    let indent_width = indent.chars().count();
    let available_width = width
        .saturating_sub(indent_width)
        .max(CODE_BLOCK_PADDING_X * 2 + 1);
    let inner_width = available_width
        .saturating_sub(CODE_BLOCK_PADDING_X * 2)
        .max(1);
    let block_style = Style::default().bg(CODE_BLOCK_BG);

    let mut rendered = Vec::with_capacity(styled_code_lines.len() + 3);
    rendered.push(Line::from(vec![
        Span::raw(indent.to_string()),
        Span::styled(" ".repeat(available_width), block_style),
    ]));

    let header = code_block_header(fence, inner_width);
    rendered.push(Line::from(vec![
        Span::raw(indent.to_string()),
        Span::styled(" ".repeat(CODE_BLOCK_PADDING_X), block_style),
        Span::styled(
            pad_right(&header, inner_width),
            Style::default().fg(CODE_BLOCK_HEADER_FG).bg(CODE_BLOCK_BG),
        ),
        Span::styled(" ".repeat(CODE_BLOCK_PADDING_X), block_style),
    ]));

    rendered.push(Line::from(vec![
        Span::raw(indent.to_string()),
        Span::styled(" ".repeat(CODE_BLOCK_PADDING_X), block_style),
        Span::styled(
            "─".repeat(inner_width),
            Style::default()
                .fg(CODE_BLOCK_SEPARATOR_FG)
                .bg(CODE_BLOCK_BG),
        ),
        Span::styled(" ".repeat(CODE_BLOCK_PADDING_X), block_style),
    ]));

    for spans in styled_code_lines {
        let (clipped_spans, content_width) = truncate_spans_to_width(spans, inner_width);

        let mut line_spans = Vec::with_capacity(clipped_spans.len() + 3);
        line_spans.push(Span::raw(indent.to_string()));
        line_spans.push(Span::styled(" ".repeat(CODE_BLOCK_PADDING_X), block_style));

        for span in clipped_spans {
            line_spans.push(Span::styled(
                span.content.into_owned(),
                span.style.bg(CODE_BLOCK_BG),
            ));
        }

        line_spans.push(Span::styled(
            " ".repeat(inner_width.saturating_sub(content_width)),
            block_style,
        ));
        line_spans.push(Span::styled(" ".repeat(CODE_BLOCK_PADDING_X), block_style));
        rendered.push(Line::from(line_spans));
    }

    if code_lines.is_empty() {
        rendered.push(Line::from(vec![
            Span::raw(indent.to_string()),
            Span::styled(" ".repeat(available_width), block_style),
        ]));
    }

    rendered.push(Line::from(vec![
        Span::raw(indent.to_string()),
        Span::styled(" ".repeat(available_width), block_style),
    ]));

    rendered
}

fn parse_location_hint(metadata: &str) -> Option<String> {
    let trimmed = metadata.trim();
    if trimmed.is_empty() {
        return None;
    }

    for key in ["file", "path", "filename", "location", "title"] {
        if let Some(value) = extract_key_value(trimmed, key) {
            return Some(value);
        }
    }

    Some(trimmed.to_string())
}

fn extract_key_value(metadata: &str, key: &str) -> Option<String> {
    let prefix = format!("{key}=");
    let start = metadata.find(&prefix)? + prefix.len();
    let value = metadata[start..].trim_start();

    if let Some(stripped) = value.strip_prefix('"') {
        let end = stripped.find('"')?;
        return Some(stripped[..end].to_string());
    }

    let token = value.split_whitespace().next()?;
    Some(token.trim_end_matches(',').to_string())
}

fn code_block_header(fence: &CodeFence, max_width: usize) -> String {
    let language = if fence.language.trim().is_empty() {
        "text".to_string()
    } else {
        fence.language.trim().to_string()
    };

    let label = if let Some(location) = fence.location_hint.as_deref() {
        format!("{language}  {location}")
    } else {
        language
    };

    truncate_to_width(&label, max_width)
}

fn truncate_spans_to_width(
    spans: Vec<Span<'static>>,
    max_width: usize,
) -> (Vec<Span<'static>>, usize) {
    let mut clipped = Vec::new();
    let mut used = 0;

    for span in spans {
        if used >= max_width {
            break;
        }

        let text = span.content.into_owned();
        let span_len = text.chars().count();
        if span_len <= max_width.saturating_sub(used) {
            used += span_len;
            clipped.push(Span::styled(text, span.style));
            continue;
        }

        let keep = max_width.saturating_sub(used);
        if keep > 0 {
            let partial = text.chars().take(keep).collect::<String>();
            used += keep;
            clipped.push(Span::styled(partial, span.style));
        }
    }

    (clipped, used)
}

fn truncate_to_width(text: &str, max_width: usize) -> String {
    text.chars().take(max_width).collect()
}

fn pad_right(text: &str, width: usize) -> String {
    let visible = text.chars().count();
    if visible >= width {
        return text.to_string();
    }

    format!("{text}{}", " ".repeat(width - visible))
}

fn highlight_code_block(code_lines: &[String], language: &str) -> Option<Vec<Vec<Span<'static>>>> {
    let syntax_set = syntax_set();
    let theme = theme()?;
    let syntax = resolve_syntax(syntax_set, language)?;
    let mut highlighter = HighlightLines::new(syntax, theme);

    let mut rendered = Vec::with_capacity(code_lines.len());
    for line in code_lines {
        let ranges = highlighter.highlight_line(line, syntax_set).ok()?;
        let mut spans = Vec::new();
        for (style, segment) in ranges {
            let tui_style = translate_style(style).unwrap_or_else(|_| Style::default());
            spans.push(Span::styled(segment.to_string(), tui_style));
        }
        rendered.push(spans);
    }

    Some(rendered)
}

fn syntax_set() -> &'static SyntaxSet {
    static INSTANCE: OnceLock<SyntaxSet> = OnceLock::new();
    INSTANCE.get_or_init(SyntaxSet::load_defaults_newlines)
}

fn theme() -> Option<&'static Theme> {
    static INSTANCE: OnceLock<Option<Theme>> = OnceLock::new();
    INSTANCE
        .get_or_init(|| {
            let theme_set = ThemeSet::load_defaults();
            theme_set
                .themes
                .get("base16-ocean.light")
                .or_else(|| theme_set.themes.values().next())
                .cloned()
        })
        .as_ref()
}

fn resolve_syntax<'a>(syntax_set: &'a SyntaxSet, language: &str) -> Option<&'a SyntaxReference> {
    let normalized = normalize_language(language);
    if normalized.is_empty() {
        return None;
    }

    syntax_set
        .find_syntax_by_token(&normalized)
        .or_else(|| syntax_set.find_syntax_by_extension(&normalized))
        .or_else(|| syntax_set.find_syntax_by_name(&normalized))
}

fn normalize_language(language: &str) -> String {
    let lowered = language.to_ascii_lowercase();
    match lowered.as_str() {
        "rs" => "rust".to_string(),
        "py" => "python".to_string(),
        "js" | "jsx" => "javascript".to_string(),
        "ts" | "tsx" => "typescript".to_string(),
        "sh" | "zsh" | "shell" => "bash".to_string(),
        "yml" => "yaml".to_string(),
        _ => lowered,
    }
}

fn parse_inline_markdown_spans(text: &str) -> Vec<Span<'static>> {
    let mut spans = Vec::new();
    let mut chars = text.chars().peekable();
    let mut current = String::new();

    while let Some(ch) = chars.next() {
        if ch == '*' && chars.peek() == Some(&'*') {
            chars.next();
            if !current.is_empty() {
                spans.push(Span::raw(std::mem::take(&mut current)));
            }

            let mut bold_text = String::new();
            loop {
                match chars.next() {
                    Some('*') if chars.peek() == Some(&'*') => {
                        chars.next();
                        break;
                    }
                    Some(c) => bold_text.push(c),
                    None => {
                        bold_text.insert(0, '*');
                        bold_text.insert(0, '*');
                        spans.push(Span::raw(bold_text));
                        return spans;
                    }
                }
            }
            spans.push(Span::styled(bold_text, Style::default().bold()));
        } else if ch == '`' {
            if !current.is_empty() {
                spans.push(Span::raw(std::mem::take(&mut current)));
            }

            let mut code_text = String::new();
            loop {
                match chars.next() {
                    Some('`') => break,
                    Some(c) => code_text.push(c),
                    None => {
                        code_text.insert(0, '`');
                        spans.push(Span::raw(code_text));
                        return spans;
                    }
                }
            }
            spans.push(Span::styled(code_text, Style::default().fg(Color::Yellow)));
        } else {
            current.push(ch);
        }
    }

    if !current.is_empty() {
        spans.push(Span::raw(current));
    }

    spans
}

fn wrap_spans(spans: &[Span<'static>], width: usize) -> Vec<Vec<Span<'static>>> {
    if width == 0 {
        return vec![spans.to_vec()];
    }

    let mut lines = Vec::new();
    let mut current_line = Vec::new();
    let mut current_line_len = 0;

    for span in spans {
        let span_style = span.style;
        let span_text = span.content.as_ref();

        for word in span_text.split_whitespace() {
            let word_len = word.chars().count();
            let space_needed = if current_line_len > 0 { 1 } else { 0 };

            if current_line_len + space_needed + word_len > width && !current_line.is_empty() {
                lines.push(std::mem::take(&mut current_line));
                current_line_len = 0;
            }

            if current_line_len > 0 {
                current_line.push(Span::raw(" "));
                current_line_len += 1;
            }

            current_line.push(Span::styled(word.to_string(), span_style));
            current_line_len += word_len;
        }
    }

    if !current_line.is_empty() {
        lines.push(current_line);
    }

    if lines.is_empty() {
        lines.push(vec![]);
    }

    lines
}

#[cfg(test)]
mod tests {
    use super::{CODE_BLOCK_BG, markdown_to_lines_with_indent};

    const TEST_INDENT: &str = "    ";

    fn line_text(line: &ratatui::text::Line<'_>) -> String {
        line.spans
            .iter()
            .map(|span| span.content.as_ref())
            .collect::<String>()
    }

    #[test]
    fn hides_fence_markers() {
        let lines = markdown_to_lines_with_indent("```rust\nlet x = 1;\n```", 120, TEST_INDENT);
        let rendered: Vec<String> = lines.iter().map(line_text).collect();

        assert!(rendered.iter().any(|line| line.contains("let x = 1;")));
        assert!(rendered.iter().all(|line| !line.contains("```")));
    }

    #[test]
    fn preserves_code_indentation() {
        let lines = markdown_to_lines_with_indent(
            "```rust\nif state {\n    .iter()\n}\n```",
            120,
            TEST_INDENT,
        );
        let rendered: Vec<String> = lines.iter().map(line_text).collect();

        assert!(rendered.iter().any(|line| line.contains("    .iter()")));
    }

    #[test]
    fn applies_syntax_highlighting_when_language_is_known() {
        let lines = markdown_to_lines_with_indent("```rust\nlet value = 1;\n```", 120, TEST_INDENT);
        let code_line = lines
            .iter()
            .find(|line| line_text(line).contains("let value = 1;"))
            .expect("expected highlighted code line");

        let keyword = code_line
            .spans
            .iter()
            .find(|span| span.content.as_ref().contains("let"))
            .expect("expected keyword token");
        let identifier = code_line
            .spans
            .iter()
            .find(|span| span.content.as_ref().contains("value"))
            .expect("expected identifier token");

        assert_ne!(keyword.style.fg, identifier.style.fg);
    }

    #[test]
    fn falls_back_to_plain_rendering_for_unknown_language() {
        let lines = markdown_to_lines_with_indent("```weirdlang\nline\n```", 120, TEST_INDENT);
        let rendered: Vec<String> = lines.iter().map(line_text).collect();

        assert!(rendered.iter().any(|line| line.contains("line")));
    }

    #[test]
    fn renders_code_blocks_with_background_box() {
        let lines = markdown_to_lines_with_indent("```rust\nlet x = 1;\n```", 40, TEST_INDENT);

        assert!(lines.len() >= 5);

        let top_line = &lines[0];
        let header_line = &lines[1];
        let separator_line = &lines[2];
        let middle_line = &lines[3];
        let bottom_line = lines.last().expect("expected closing line");

        assert!(
            top_line
                .spans
                .iter()
                .any(|span| span.style.bg == Some(CODE_BLOCK_BG))
        );
        assert!(
            bottom_line
                .spans
                .iter()
                .any(|span| span.style.bg == Some(CODE_BLOCK_BG))
        );
        assert!(line_text(header_line).contains("rust"));
        assert!(line_text(separator_line).contains("─"));

        let code_spans_with_bg = middle_line
            .spans
            .iter()
            .filter(|span| !span.content.is_empty())
            .skip(1)
            .all(|span| span.style.bg == Some(CODE_BLOCK_BG));
        assert!(code_spans_with_bg);
    }

    #[test]
    fn shows_location_hint_in_code_block_header() {
        let markdown = "```rust src/cli/chat.rs:508\nlet x = 1;\n```";
        let lines = markdown_to_lines_with_indent(markdown, 80, TEST_INDENT);
        let rendered: Vec<String> = lines.iter().map(line_text).collect();

        assert!(
            rendered
                .iter()
                .any(|line| line.contains("rust  src/cli/chat.rs:508"))
        );
    }
}
