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

const INDENT: &str = "  ";

pub fn markdown_to_lines(markdown: &str, width: usize) -> Vec<Line<'static>> {
    let mut rendered = Vec::new();
    let mut in_code_block = false;
    let mut code_language = String::new();
    let mut code_lines: Vec<String> = Vec::new();

    for line in markdown.lines() {
        if let Some(language) = fence_language(line) {
            if in_code_block {
                rendered.extend(render_code_block(&code_lines, &code_language));
                in_code_block = false;
                code_language.clear();
                code_lines.clear();
            } else {
                in_code_block = true;
                code_language = language;
            }
            continue;
        }

        if in_code_block {
            code_lines.push(line.to_string());
            continue;
        }

        rendered.extend(render_text_line(line, width));
    }

    if in_code_block {
        rendered.extend(render_code_block(&code_lines, &code_language));
    }

    rendered
}

fn fence_language(line: &str) -> Option<String> {
    let trimmed = line.trim_start();
    if !trimmed.starts_with("```") {
        return None;
    }

    let language = trimmed
        .trim_start_matches("```")
        .split_whitespace()
        .next()
        .unwrap_or("");
    Some(language.to_string())
}

fn render_text_line(line: &str, width: usize) -> Vec<Line<'static>> {
    let spans = parse_inline_markdown_spans(line);
    let wrapped = wrap_spans(&spans, width.saturating_sub(INDENT.chars().count()));

    wrapped
        .into_iter()
        .map(|wrapped_line| {
            let mut indented = vec![Span::raw(INDENT)];
            indented.extend(wrapped_line);
            Line::from(indented)
        })
        .collect()
}

fn render_code_block(code_lines: &[String], language: &str) -> Vec<Line<'static>> {
    if let Some(highlighted) = highlight_code_block(code_lines, language) {
        return highlighted;
    }

    code_lines
        .iter()
        .map(|line| Line::from(vec![Span::raw(INDENT), Span::raw(line.clone())]))
        .collect()
}

fn highlight_code_block(code_lines: &[String], language: &str) -> Option<Vec<Line<'static>>> {
    let syntax_set = syntax_set();
    let theme = theme();
    let syntax = resolve_syntax(syntax_set, language)?;
    let mut highlighter = HighlightLines::new(syntax, theme);

    let mut rendered = Vec::with_capacity(code_lines.len());
    for line in code_lines {
        let ranges = highlighter.highlight_line(line, syntax_set).ok()?;
        let mut spans = vec![Span::raw(INDENT)];
        for (style, segment) in ranges {
            let tui_style = translate_style(style).unwrap_or_else(|_| Style::default());
            spans.push(Span::styled(segment.to_string(), tui_style));
        }
        rendered.push(Line::from(spans));
    }

    Some(rendered)
}

fn syntax_set() -> &'static SyntaxSet {
    static INSTANCE: OnceLock<SyntaxSet> = OnceLock::new();
    INSTANCE.get_or_init(SyntaxSet::load_defaults_newlines)
}

fn theme() -> &'static Theme {
    static INSTANCE: OnceLock<Theme> = OnceLock::new();
    INSTANCE.get_or_init(|| {
        let theme_set = ThemeSet::load_defaults();
        theme_set
            .themes
            .get("base16-ocean.light")
            .or_else(|| theme_set.themes.values().next())
            .expect("syntect should provide at least one bundled theme")
            .clone()
    })
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
    use super::markdown_to_lines;

    fn line_text(line: &ratatui::text::Line<'_>) -> String {
        line.spans
            .iter()
            .map(|span| span.content.as_ref())
            .collect::<String>()
    }

    #[test]
    fn hides_fence_markers() {
        let lines = markdown_to_lines("```rust\nlet x = 1;\n```", 120);
        let rendered: Vec<String> = lines.iter().map(line_text).collect();

        assert!(rendered.iter().any(|line| line.contains("let x = 1;")));
        assert!(rendered.iter().all(|line| !line.contains("```")));
    }

    #[test]
    fn preserves_code_indentation() {
        let lines = markdown_to_lines("```rust\nif state {\n    .iter()\n}\n```", 120);
        let rendered: Vec<String> = lines.iter().map(line_text).collect();

        assert!(rendered.iter().any(|line| line == "      .iter()"));
    }

    #[test]
    fn applies_syntax_highlighting_when_language_is_known() {
        let lines = markdown_to_lines("```rust\nlet value = 1;\n```", 120);
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
        let lines = markdown_to_lines("```weirdlang\nline\n```", 120);
        let rendered: Vec<String> = lines.iter().map(line_text).collect();

        assert!(rendered.iter().any(|line| line.contains("line")));
    }
}
