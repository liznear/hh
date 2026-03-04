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
    let lines: Vec<&str> = markdown.lines().collect();
    let mut index = 0;

    while index < lines.len() {
        let line = lines[index];
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
            index += 1;
            continue;
        }

        if in_code_block {
            code_lines.push(line.to_string());
            index += 1;
            continue;
        }

        if let Some((table_lines, consumed)) = parse_table(&lines[index..]) {
            rendered.extend(render_table(&table_lines, width, indent));
            index += consumed;
            continue;
        }

        rendered.extend(render_text_line(line, width, indent));
        index += 1;
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

#[derive(Clone)]
struct TableRow {
    cells: Vec<String>,
}

fn parse_table(lines: &[&str]) -> Option<(Vec<TableRow>, usize)> {
    if lines.len() < 2 {
        return None;
    }

    let header = parse_table_row(lines[0])?;
    let separator = parse_table_row(lines[1])?;
    if header.cells.is_empty() || separator.cells.len() != header.cells.len() {
        return None;
    }

    if !separator
        .cells
        .iter()
        .all(|cell| is_table_separator_cell(cell))
    {
        return None;
    }

    let mut rows = vec![header];
    let mut consumed = 2;

    for line in &lines[2..] {
        let Some(row) = parse_table_row(line) else {
            break;
        };
        if row.cells.len() != rows[0].cells.len() {
            break;
        }
        rows.push(row);
        consumed += 1;
    }

    Some((rows, consumed))
}

fn parse_table_row(line: &str) -> Option<TableRow> {
    let trimmed = line.trim();
    if trimmed.len() < 2 || !trimmed.contains('|') {
        return None;
    }

    let inner = trimmed
        .strip_prefix('|')
        .and_then(|line| line.strip_suffix('|'))
        .unwrap_or(trimmed);
    let cells = inner
        .split('|')
        .map(|cell| cell.trim().to_string())
        .collect();

    Some(TableRow { cells })
}

fn is_table_separator_cell(cell: &str) -> bool {
    let trimmed = cell.trim();
    !trimmed.is_empty()
        && trimmed.chars().all(|ch| matches!(ch, '-' | ':' | ' '))
        && trimmed.chars().filter(|ch| *ch == '-').count() >= 3
}

fn render_table(rows: &[TableRow], width: usize, indent: &str) -> Vec<Line<'static>> {
    if rows.is_empty() {
        return Vec::new();
    }

    let column_count = rows[0].cells.len();
    let mut column_widths = vec![3; column_count];
    for row in rows {
        for (index, cell) in row.cells.iter().enumerate() {
            column_widths[index] = column_widths[index].max(cell.chars().count());
        }
    }

    let indent_width = indent.chars().count();
    let min_table_width = column_count * 4 + 1;
    let max_table_width = width.saturating_sub(indent_width).max(min_table_width);
    shrink_column_widths(&mut column_widths, max_table_width);

    let mut rendered = Vec::with_capacity(rows.len() * 2 + 3);
    rendered.push(render_table_border(
        indent,
        &column_widths,
        '┌',
        '┬',
        '┐',
        '─',
    ));
    rendered.extend(render_table_row(indent, &rows[0], &column_widths, true));

    if rows.len() > 1 {
        rendered.push(render_table_border(
            indent,
            &column_widths,
            '╞',
            '╪',
            '╡',
            '═',
        ));
        for (index, row) in rows[1..].iter().enumerate() {
            rendered.extend(render_table_row(indent, row, &column_widths, false));
            if index + 1 != rows.len() - 1 {
                rendered.push(render_table_border(
                    indent,
                    &column_widths,
                    '├',
                    '┼',
                    '┤',
                    '─',
                ));
            }
        }
    }

    rendered.push(render_table_border(
        indent,
        &column_widths,
        '└',
        '┴',
        '┘',
        '─',
    ));
    rendered
}

fn shrink_column_widths(widths: &mut [usize], max_table_width: usize) {
    let min_width = 3;
    while table_total_width(widths) > max_table_width {
        let Some((index, current_width)) = widths
            .iter()
            .copied()
            .enumerate()
            .max_by_key(|(_, width)| *width)
        else {
            break;
        };

        if current_width <= min_width {
            break;
        }

        widths[index] -= 1;
    }
}

fn table_total_width(widths: &[usize]) -> usize {
    widths.iter().sum::<usize>() + widths.len() * 3 + 1
}

fn render_table_border(
    indent: &str,
    widths: &[usize],
    left: char,
    middle: char,
    right: char,
    fill: char,
) -> Line<'static> {
    let mut text = String::with_capacity(indent.len() + table_total_width(widths));
    text.push_str(indent);
    text.push(left);
    for (index, width) in widths.iter().enumerate() {
        text.push_str(&fill.to_string().repeat(*width + 2));
        text.push(if index + 1 == widths.len() {
            right
        } else {
            middle
        });
    }
    Line::raw(text)
}

fn render_table_row(
    indent: &str,
    row: &TableRow,
    widths: &[usize],
    header: bool,
) -> Vec<Line<'static>> {
    let wrapped_cells: Vec<Vec<Vec<Span<'static>>>> = row
        .cells
        .iter()
        .zip(widths.iter())
        .map(|(cell, width)| {
            let mut wrapped = wrap_spans(&parse_inline_markdown_spans(cell), *width);
            if header {
                for line in &mut wrapped {
                    for span in line {
                        *span = Span::styled(span.content.clone().into_owned(), span.style.bold());
                    }
                }
            }
            wrapped
        })
        .collect();

    let row_height = wrapped_cells.iter().map(Vec::len).max().unwrap_or(1);
    let mut rendered = Vec::with_capacity(row_height);

    for line_index in 0..row_height {
        let mut spans = vec![Span::raw(indent.to_string()), Span::raw("│ ")];

        for (column_index, width) in widths.iter().enumerate() {
            let cell_line = wrapped_cells
                .get(column_index)
                .and_then(|cell| cell.get(line_index))
                .cloned()
                .unwrap_or_default();
            let used = spans_width(&cell_line);

            spans.extend(cell_line);
            spans.push(Span::raw(" ".repeat(width.saturating_sub(used))));
            spans.push(Span::raw(if column_index + 1 == widths.len() {
                " │"
            } else {
                " │ "
            }));
        }

        rendered.push(Line::from(spans));
    }

    rendered
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

fn spans_width(spans: &[Span<'static>]) -> usize {
    spans.iter().map(|span| span.content.chars().count()).sum()
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
    use ratatui::style::Color;

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

    #[test]
    fn renders_markdown_tables_with_borders() {
        let markdown = "| Name | Value |\n| --- | --- |\n| foo | bar |";
        let lines = markdown_to_lines_with_indent(markdown, 80, TEST_INDENT);
        let rendered: Vec<String> = lines.iter().map(line_text).collect();

        assert!(rendered.iter().any(|line| line.contains("┌")));
        assert!(rendered.iter().any(|line| line.contains("│ Name")));
        assert!(rendered.iter().any(|line| line.contains("│ foo")));
        assert!(rendered.iter().any(|line| line.contains("└")));
    }

    #[test]
    fn renders_double_separator_after_table_header() {
        let markdown = "| Name | Value |\n| --- | --- |\n| foo | bar |";
        let lines = markdown_to_lines_with_indent(markdown, 80, TEST_INDENT);
        let rendered: Vec<String> = lines.iter().map(line_text).collect();
        let double_separator_count = rendered.iter().filter(|line| line.contains("╞")).count();
        let single_separator_count = rendered.iter().filter(|line| line.contains("├")).count();

        assert_eq!(double_separator_count, 1);
        assert_eq!(single_separator_count, 0);
        assert!(rendered.iter().any(|line| line.contains("═")));
    }

    #[test]
    fn renders_separator_between_table_data_rows() {
        let markdown = "| Name | Value |\n| --- | --- |\n| foo | bar |\n| baz | qux |";
        let lines = markdown_to_lines_with_indent(markdown, 80, TEST_INDENT);
        let rendered: Vec<String> = lines.iter().map(line_text).collect();
        let double_separator_count = rendered.iter().filter(|line| line.contains("╞")).count();
        let single_separator_count = rendered.iter().filter(|line| line.contains("├")).count();

        assert_eq!(double_separator_count, 1);
        assert_eq!(single_separator_count, 1);
    }

    #[test]
    fn wraps_table_cells_instead_of_truncating() {
        let markdown = "| Name | Value |\n| --- | --- |\n| foo | this cell should wrap across multiple lines |";
        let lines = markdown_to_lines_with_indent(markdown, 24, TEST_INDENT);
        let rendered: Vec<String> = lines.iter().map(line_text).collect();

        assert!(rendered.iter().any(|line| line.contains("this")));
        assert!(rendered.iter().any(|line| line.contains("cell")));
        assert!(rendered.iter().any(|line| line.contains("should")));
        assert!(rendered.iter().any(|line| line.contains("multiple")));
    }

    #[test]
    fn keeps_inline_markdown_inside_table_cells() {
        let markdown = "| Header | Value |\n| --- | --- |\n| **bold** | `code` |";
        let lines = markdown_to_lines_with_indent(markdown, 80, TEST_INDENT);
        let row = lines
            .iter()
            .find(|line| line_text(line).contains("bold") && line_text(line).contains("code"))
            .expect("expected table row");

        assert!(row.spans.iter().all(|span| !span.content.contains("**")));
        assert!(row.spans.iter().all(|span| !span.content.contains('`')));
        assert!(row.spans.iter().any(|span| {
            span.content.contains("bold")
                && span
                    .style
                    .add_modifier
                    .contains(ratatui::style::Modifier::BOLD)
        }));
        assert!(
            row.spans
                .iter()
                .any(|span| span.content.contains("code") && span.style.fg == Some(Color::Yellow))
        );
    }
}
