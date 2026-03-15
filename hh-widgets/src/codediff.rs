use std::sync::OnceLock;

use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::{Color, Style},
    widgets::Widget,
};
use syntect::{
    easy::HighlightLines,
    highlighting::{Theme, ThemeSet},
    parsing::{SyntaxReference, SyntaxSet},
};
use syntect_tui::translate_style;

const DEFAULT_MAX_RENDERED_LINES: usize = 120;
const DEFAULT_MAX_RENDERED_CHARS: usize = 8_000;
const DEFAULT_SHOW_FILE_HEADERS: bool = true;
const DIFF_LINE_NUMBER_WIDTH: usize = 4;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CodeDiffLayout {
    Unified,
    #[default]
    SideBySide,
}

/// Parsed and renderable unified diff widget.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct CodeDiff {
    rendered: CodeDiffRender,
    styles: CodeDiffStyles,
    frame: CodeDiffFrame,
    layout: CodeDiffLayout,
}

impl CodeDiff {
    pub fn from_unified_diff(unified_diff: impl Into<String>) -> Self {
        let block = CodeDiffBlock {
            unified_diff: unified_diff.into(),
        };
        let options = CodeDiffOptions::default();
        let rendered = render_unified_diff(&block, &options);

        Self {
            rendered,
            styles: CodeDiffStyles::default(),
            frame: CodeDiffFrame::default(),
            layout: CodeDiffLayout::SideBySide,
        }
    }

    pub fn with_styles(mut self, styles: CodeDiffStyles) -> Self {
        self.styles = styles;
        self
    }

    pub fn with_layout(mut self, layout: CodeDiffLayout) -> Self {
        self.layout = layout;
        self
    }

    pub fn with_frame(mut self, frame: CodeDiffFrame) -> Self {
        self.frame = frame;
        self
    }

    pub fn with_padding(mut self, horizontal: u16, vertical: u16) -> Self {
        self.frame.padding_horizontal = horizontal;
        self.frame.padding_vertical = vertical;
        self
    }

    pub fn with_panel_style(mut self, panel: Style) -> Self {
        self.frame.panel = panel;
        self
    }

    pub fn rendered_lines(&self) -> &[CodeDiffLine] {
        &self.rendered.lines
    }

    pub fn is_truncated(&self) -> bool {
        self.rendered.truncated
    }

    pub fn measured_height(&self) -> u16 {
        let mut lines = match self.layout {
            CodeDiffLayout::Unified => self.rendered.lines.len(),
            CodeDiffLayout::SideBySide => side_by_side_row_count(&self.rendered.lines),
        };
        if self.rendered.truncated {
            lines = lines.saturating_add(1);
        }
        let content_height = u16::try_from(lines).unwrap_or(u16::MAX);
        content_height.saturating_add(self.frame.padding_vertical.saturating_mul(2))
    }
}

impl Widget for CodeDiff {
    fn render(self, area: Rect, buf: &mut Buffer)
    where
        Self: Sized,
    {
        (&self).render(area, buf);
    }
}

impl Widget for &CodeDiff {
    fn render(self, area: Rect, buf: &mut Buffer)
    where
        Self: Sized,
    {
        if area.width == 0 || area.height == 0 {
            return;
        }

        fill_background(buf, area, self.frame.panel);

        let Some(content_area) = inner_content_area(
            area,
            self.frame.padding_horizontal,
            self.frame.padding_vertical,
        ) else {
            return;
        };

        let width = usize::from(content_area.width);
        let max_rows = usize::from(content_area.height);
        let rendered_rows = match self.layout {
            CodeDiffLayout::Unified => {
                self.render_unified_rows(buf, content_area, width, max_rows)
            }
            CodeDiffLayout::SideBySide => {
                self.render_side_by_side_rows(buf, content_area, width, max_rows)
            }
        };

        if self.rendered.truncated && rendered_rows < max_rows {
            let suffix = truncate_chars("... diff truncated", width);
            buf.set_stringn(
                content_area.x,
                content_area.y.saturating_add(rendered_rows as u16),
                suffix,
                width,
                self.styles.truncated,
            );
        }
    }
}

impl CodeDiff {
    fn render_unified_rows(
        &self,
        buf: &mut Buffer,
        area: Rect,
        width: usize,
        max_rows: usize,
    ) -> usize {
        let mut row = 0usize;
        let mut language: Option<String> = None;
        for line in &self.rendered.lines {
            if row >= max_rows {
                break;
            }
            if let Some(detected) = extract_language_from_header(&line.text) {
                language = Some(detected);
            }
            let shown = truncate_chars(&line.text, width);
            let style = self.styles.style_for(line.kind);
            buf.set_stringn(
                area.x,
                area.y.saturating_add(row as u16),
                shown,
                width,
                style,
            );
            if line.kind != CodeDiffLineKind::Meta {
                write_highlighted_segments(
                    buf,
                    area.x,
                    area.y.saturating_add(row as u16),
                    width,
                    style,
                    &line.text,
                    language.as_deref(),
                );
            }
            row = row.saturating_add(1);
        }
        row
    }

    fn render_side_by_side_rows(
        &self,
        buf: &mut Buffer,
        area: Rect,
        width: usize,
        max_rows: usize,
    ) -> usize {
        let (left_width, right_width) = diff_column_widths(width);
        if left_width == 0 || right_width == 0 {
            return self.render_unified_rows(buf, area, width, max_rows);
        }

        let mut row = 0usize;
        let mut lines = self
            .rendered
            .lines
            .iter()
            .map(|line| line.text.as_str())
            .peekable();
        let mut cursor = DiffLineCursor::default();
        let mut language: Option<String> = None;

        while let Some(side_by_side) = next_diff_row(&mut lines, &mut cursor) {
            if row >= max_rows {
                break;
            }

            if matches!(side_by_side.kind, SideBySideDiffKind::Meta)
                && side_by_side
                    .left
                    .as_ref()
                    .is_some_and(|cell| is_file_header(&cell.text))
            {
                if let Some(meta_text) = side_by_side.left.as_ref().map(|cell| cell.text.as_str())
                    && let Some(detected) = extract_language_from_header(meta_text)
                {
                    language = Some(detected);
                }
                continue;
            }

            let y = area.y.saturating_add(row as u16);

            if matches!(side_by_side.kind, SideBySideDiffKind::Meta) {
                let meta_text = side_by_side
                    .left
                    .as_ref()
                    .or(side_by_side.right.as_ref())
                    .map(|cell| cell.text.as_str())
                    .unwrap_or("");
                if let Some(detected) = extract_language_from_header(meta_text) {
                    language = Some(detected);
                }
                let shown = truncate_for_column(meta_text, width);
                buf.set_stringn(area.x, y, shown, width, self.styles.meta);
                row = row.saturating_add(1);
                continue;
            }

            let left_text = render_diff_cell(side_by_side.left.as_ref(), left_width);
            let right_text = render_diff_cell(side_by_side.right.as_ref(), right_width);
            let (left_style, right_style) = self.styles.side_by_side_styles(side_by_side.kind);

            buf.set_stringn(area.x, y, left_text, left_width, left_style);
            buf.set_stringn(
                area.x.saturating_add(left_width as u16),
                y,
                right_text,
                right_width,
                right_style,
            );

            if let Some(left_cell) = side_by_side.left.as_ref() {
                write_diff_cell_prefix(
                    buf,
                    area.x,
                    y,
                    left_cell,
                    left_width,
                    &self.styles,
                );
            }
            if let Some(right_cell) = side_by_side.right.as_ref() {
                write_diff_cell_prefix(
                    buf,
                    area.x.saturating_add(left_width as u16),
                    y,
                    right_cell,
                    right_width,
                    &self.styles,
                );
            }

            if let Some(left_cell) = side_by_side.left.as_ref() {
                write_diff_cell_highlighted(
                    buf,
                    area.x,
                    y,
                    left_cell,
                    left_width,
                    left_style,
                    language.as_deref(),
                );
            }
            if let Some(right_cell) = side_by_side.right.as_ref() {
                write_diff_cell_highlighted(
                    buf,
                    area.x.saturating_add(left_width as u16),
                    y,
                    right_cell,
                    right_width,
                    right_style,
                    language.as_deref(),
                );
            }

            row = row.saturating_add(1);
        }

        row
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CodeDiffStyles {
    pub add: Style,
    pub remove: Style,
    pub add_prefix: Style,
    pub remove_prefix: Style,
    pub meta: Style,
    pub context: Style,
    pub truncated: Style,
}

impl Default for CodeDiffStyles {
    fn default() -> Self {
        Self {
            add: Style::default().bg(Color::Rgb(218, 251, 225)),
            remove: Style::default().bg(Color::Rgb(255, 235, 233)),
            add_prefix: Style::default().fg(Color::Black).bg(Color::Rgb(172, 238, 187)),
            remove_prefix: Style::default().fg(Color::Black).bg(Color::Rgb(255, 206, 203)),
            meta: Style::default().fg(Color::Cyan),
            context: Style::default(),
            truncated: Style::default().fg(Color::DarkGray),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CodeDiffFrame {
    pub panel: Style,
    pub padding_horizontal: u16,
    pub padding_vertical: u16,
}

impl Default for CodeDiffFrame {
    fn default() -> Self {
        Self {
            panel: Style::default().bg(Color::Rgb(240, 242, 245)),
            padding_horizontal: 3,
            padding_vertical: 1,
        }
    }
}

fn fill_background(buf: &mut Buffer, area: Rect, style: Style) {
    let row = " ".repeat(usize::from(area.width));
    for y in 0..area.height {
        buf.set_stringn(area.x, area.y.saturating_add(y), row.as_str(), row.len(), style);
    }
}

fn inner_content_area(area: Rect, padding_horizontal: u16, padding_vertical: u16) -> Option<Rect> {
    let horizontal = padding_horizontal.saturating_mul(2);
    let vertical = padding_vertical.saturating_mul(2);
    if area.width <= horizontal || area.height <= vertical {
        return None;
    }

    Some(Rect::new(
        area.x.saturating_add(padding_horizontal),
        area.y.saturating_add(padding_vertical),
        area.width.saturating_sub(horizontal),
        area.height.saturating_sub(vertical),
    ))
}

impl CodeDiffStyles {
    fn style_for(&self, kind: CodeDiffLineKind) -> Style {
        match kind {
            CodeDiffLineKind::Add => self.add,
            CodeDiffLineKind::Remove => self.remove,
            CodeDiffLineKind::Meta => self.meta,
            CodeDiffLineKind::Context => self.context,
        }
    }

    fn side_by_side_styles(&self, kind: SideBySideDiffKind) -> (Style, Style) {
        match kind {
            SideBySideDiffKind::Context => (self.context, self.context),
            SideBySideDiffKind::Removed => (self.remove, self.context),
            SideBySideDiffKind::Added => (self.context, self.add),
            SideBySideDiffKind::Meta => (self.meta, self.meta),
            SideBySideDiffKind::Changed => (self.remove, self.add),
        }
    }

    fn prefix_style_for_marker(&self, marker: char) -> Option<Style> {
        match marker {
            '+' => Some(self.add_prefix),
            '-' => Some(self.remove_prefix),
            _ => None,
        }
    }
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

fn extract_language_from_header(line: &str) -> Option<String> {
    let path = line
        .strip_prefix("+++ ")
        .or_else(|| line.strip_prefix("--- "))?
        .trim();
    let path = path
        .strip_prefix("a/")
        .or_else(|| path.strip_prefix("b/"))
        .unwrap_or(path);
    let path = path.split_whitespace().next().unwrap_or(path);
    let ext = path.rsplit('.').next()?;
    if ext == path || ext.is_empty() {
        return None;
    }
    Some(normalize_language(ext))
}

fn highlighted_segments(text: &str, language: Option<&str>) -> Option<Vec<(String, Style)>> {
    let language = language?;
    let syntax_set = syntax_set();
    let theme = theme()?;
    let syntax = resolve_syntax(syntax_set, language)?;
    let mut highlighter = HighlightLines::new(syntax, theme);
    let ranges = highlighter.highlight_line(text, syntax_set).ok()?;

    let mut segments = Vec::with_capacity(ranges.len());
    for (style, segment) in ranges {
        let tui_style = translate_style(style).unwrap_or_else(|_| Style::default());
        segments.push((segment.to_string(), tui_style));
    }
    Some(segments)
}

fn write_highlighted_segments(
    buf: &mut Buffer,
    x: u16,
    y: u16,
    max_width: usize,
    base_style: Style,
    text: &str,
    language: Option<&str>,
) {
    if max_width == 0 {
        return;
    }

    let Some(segments) = highlighted_segments(text, language) else {
        return;
    };

    let mut offset = 0usize;
    for (segment, seg_style) in segments {
        if offset >= max_width {
            break;
        }
        let remaining = max_width - offset;
        let part = truncate_for_column(&segment, remaining);
        let part_width = part.chars().count();
        if part_width == 0 {
            continue;
        }
        buf.set_stringn(
            x.saturating_add(offset as u16),
            y,
            part,
            remaining,
            merge_highlight_style(base_style, seg_style),
        );
        offset = offset.saturating_add(part_width);
    }
}

fn merge_highlight_style(base: Style, highlight: Style) -> Style {
    let mut merged = base.patch(highlight);
    if base.fg.is_some() {
        merged.fg = base.fg;
    }
    if base.bg.is_some() {
        merged.bg = base.bg;
    }
    merged
}

fn write_diff_cell_highlighted(
    buf: &mut Buffer,
    x: u16,
    y: u16,
    cell: &DiffCell,
    width: usize,
    base_style: Style,
    language: Option<&str>,
) {
    if width == 0 || language.is_none() {
        return;
    }

    if cell.marker.is_none() && cell.line_number.is_none() {
        write_highlighted_segments(buf, x, y, width, base_style, &cell.text, language);
        return;
    }

    let Some(prefix) = diff_cell_prefix(cell) else {
        return;
    };
    let prefix_width = prefix.chars().count();
    if width <= prefix_width {
        return;
    }

    write_highlighted_segments(
        buf,
        x.saturating_add(prefix_width as u16),
        y,
        width - prefix_width,
        base_style,
        &cell.text,
        language,
    );
}

fn write_diff_cell_prefix(
    buf: &mut Buffer,
    x: u16,
    y: u16,
    cell: &DiffCell,
    width: usize,
    styles: &CodeDiffStyles,
) {
    let Some(marker) = cell.marker else {
        return;
    };
    let Some(prefix_style) = styles.prefix_style_for_marker(marker) else {
        return;
    };
    let Some(prefix) = diff_cell_prefix(cell) else {
        return;
    };
    let clipped = truncate_for_column(&prefix, width);
    if clipped.is_empty() {
        return;
    }
    let clipped_width = clipped.chars().count();
    buf.set_stringn(x, y, clipped, clipped_width, prefix_style);
}

fn diff_cell_prefix(cell: &DiffCell) -> Option<String> {
    if cell.marker.is_none() && cell.line_number.is_none() {
        return None;
    }

    let line_number = match cell.line_number {
        Some(n) => format!("{n:>width$}", width = DIFF_LINE_NUMBER_WIDTH),
        None => " ".repeat(DIFF_LINE_NUMBER_WIDTH),
    };
    let marker = cell.marker.unwrap_or(' ');
    Some(format!("{line_number} {marker} "))
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
struct CodeDiffBlock {
    unified_diff: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct CodeDiffOptions {
    max_rendered_lines: usize,
    max_rendered_chars: usize,
    show_file_headers: bool,
}

impl Default for CodeDiffOptions {
    fn default() -> Self {
        Self {
            max_rendered_lines: DEFAULT_MAX_RENDERED_LINES,
            max_rendered_chars: DEFAULT_MAX_RENDERED_CHARS,
            show_file_headers: DEFAULT_SHOW_FILE_HEADERS,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CodeDiffLineKind {
    Add,
    Remove,
    Meta,
    Context,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CodeDiffLine {
    pub kind: CodeDiffLineKind,
    pub text: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
struct CodeDiffRender {
    pub lines: Vec<CodeDiffLine>,
    pub truncated: bool,
}

#[derive(Debug, Clone)]
struct DiffCell {
    line_number: Option<usize>,
    marker: Option<char>,
    text: String,
}

#[derive(Debug)]
struct SideBySideDiffRow {
    left: Option<DiffCell>,
    right: Option<DiffCell>,
    kind: SideBySideDiffKind,
}

#[derive(Debug, Clone, Copy)]
enum SideBySideDiffKind {
    Context,
    Removed,
    Added,
    Meta,
    Changed,
}

#[derive(Debug, Default)]
struct DiffLineCursor {
    left_line: Option<usize>,
    right_line: Option<usize>,
}

fn render_unified_diff(block: &CodeDiffBlock, options: &CodeDiffOptions) -> CodeDiffRender {
    let mut out = Vec::new();
    let mut rendered_chars = 0usize;
    let mut truncated = false;

    for (index, raw_line) in block.unified_diff.lines().enumerate() {
        let line_chars = raw_line.chars().count();
        if index >= options.max_rendered_lines
            || rendered_chars.saturating_add(line_chars) > options.max_rendered_chars
        {
            truncated = true;
            break;
        }

        let kind = classify_line_kind(raw_line, options.show_file_headers);
        if !options.show_file_headers && is_file_header(raw_line) {
            continue;
        }

        rendered_chars = rendered_chars.saturating_add(line_chars);
        out.push(CodeDiffLine {
            kind,
            text: raw_line.to_string(),
        });
    }

    CodeDiffRender {
        lines: out,
        truncated,
    }
}

fn is_file_header(line: &str) -> bool {
    line.starts_with("---") || line.starts_with("+++")
}

fn classify_line_kind(line: &str, show_file_headers: bool) -> CodeDiffLineKind {
    if line.starts_with("@@") {
        return CodeDiffLineKind::Meta;
    }
    if line.starts_with('+') && !line.starts_with("+++") {
        return CodeDiffLineKind::Add;
    }
    if line.starts_with('-') && !line.starts_with("---") {
        return CodeDiffLineKind::Remove;
    }
    if line.starts_with("+++") || line.starts_with("---") {
        if show_file_headers {
            return CodeDiffLineKind::Meta;
        }
        return CodeDiffLineKind::Context;
    }
    CodeDiffLineKind::Context
}

fn truncate_chars(input: &str, max_chars: usize) -> String {
    if input.chars().count() <= max_chars {
        return input.to_string();
    }
    input.chars().take(max_chars).collect()
}

fn side_by_side_row_count(lines: &[CodeDiffLine]) -> usize {
    let mut cursor = DiffLineCursor::default();
    let mut iter = lines.iter().map(|line| line.text.as_str()).peekable();
    let mut count = 0usize;
    while let Some(row) = next_diff_row(&mut iter, &mut cursor) {
        if matches!(row.kind, SideBySideDiffKind::Meta)
            && row
                .left
                .as_ref()
                .is_some_and(|cell| is_file_header(&cell.text))
        {
            continue;
        }
        count = count.saturating_add(1);
    }
    count
}

fn diff_column_widths(width: usize) -> (usize, usize) {
    let left = width / 2;
    let right = width.saturating_sub(left);
    (left, right)
}

fn next_diff_row<'a>(
    lines: &mut std::iter::Peekable<impl Iterator<Item = &'a str>>,
    cursor: &mut DiffLineCursor,
) -> Option<SideBySideDiffRow> {
    let raw = lines.next()?;

    if raw.starts_with("@@") || raw.starts_with("---") || raw.starts_with("+++") {
        if let Some((left, right)) = parse_hunk_line_numbers(raw) {
            cursor.left_line = Some(left);
            cursor.right_line = Some(right);
        }
        return Some(SideBySideDiffRow {
            left: Some(DiffCell {
                line_number: None,
                marker: None,
                text: raw.to_string(),
            }),
            right: Some(DiffCell {
                line_number: None,
                marker: None,
                text: raw.to_string(),
            }),
            kind: SideBySideDiffKind::Meta,
        });
    }

    if let Some(context) = raw.strip_prefix(' ') {
        return Some(SideBySideDiffRow {
            left: Some(DiffCell {
                line_number: take_next_line_number(&mut cursor.left_line),
                marker: None,
                text: context.to_string(),
            }),
            right: Some(DiffCell {
                line_number: take_next_line_number(&mut cursor.right_line),
                marker: None,
                text: context.to_string(),
            }),
            kind: SideBySideDiffKind::Context,
        });
    }

    if raw.starts_with('-') && !raw.starts_with("---") {
        if let Some(next) = lines.peek()
            && next.starts_with('+')
            && !next.starts_with("+++")
        {
            let added = lines.next().unwrap_or_default();
            return Some(SideBySideDiffRow {
                left: Some(DiffCell {
                    line_number: take_next_line_number(&mut cursor.left_line),
                    marker: Some('-'),
                    text: raw.strip_prefix('-').unwrap_or(raw).to_string(),
                }),
                right: Some(DiffCell {
                    line_number: take_next_line_number(&mut cursor.right_line),
                    marker: Some('+'),
                    text: added.strip_prefix('+').unwrap_or(added).to_string(),
                }),
                kind: SideBySideDiffKind::Changed,
            });
        }

        return Some(SideBySideDiffRow {
            left: Some(DiffCell {
                line_number: take_next_line_number(&mut cursor.left_line),
                marker: Some('-'),
                text: raw.strip_prefix('-').unwrap_or(raw).to_string(),
            }),
            right: None,
            kind: SideBySideDiffKind::Removed,
        });
    }

    if raw.starts_with('+') && !raw.starts_with("+++") {
        return Some(SideBySideDiffRow {
            left: None,
            right: Some(DiffCell {
                line_number: take_next_line_number(&mut cursor.right_line),
                marker: Some('+'),
                text: raw.strip_prefix('+').unwrap_or(raw).to_string(),
            }),
            kind: SideBySideDiffKind::Added,
        });
    }

    Some(SideBySideDiffRow {
        left: Some(DiffCell {
            line_number: None,
            marker: None,
            text: raw.to_string(),
        }),
        right: Some(DiffCell {
            line_number: None,
            marker: None,
            text: raw.to_string(),
        }),
        kind: SideBySideDiffKind::Context,
    })
}

fn parse_hunk_line_numbers(raw: &str) -> Option<(usize, usize)> {
    if !raw.starts_with("@@") {
        return None;
    }

    let mut parts = raw.split_whitespace();
    let _ = parts.next()?;
    let left = parts.next()?;
    let right = parts.next()?;

    let left_start = left
        .strip_prefix('-')?
        .split(',')
        .next()?
        .parse::<usize>()
        .ok()?;
    let right_start = right
        .strip_prefix('+')?
        .split(',')
        .next()?
        .parse::<usize>()
        .ok()?;

    Some((left_start, right_start))
}

fn take_next_line_number(line_number: &mut Option<usize>) -> Option<usize> {
    match line_number {
        Some(current) => {
            let value = *current;
            *current = current.saturating_add(1);
            Some(value)
        }
        None => None,
    }
}

fn truncate_for_column(input: &str, max_chars: usize) -> String {
    if max_chars == 0 {
        return String::new();
    }

    let mut chars = input.chars();
    let taken: String = chars.by_ref().take(max_chars).collect();
    if chars.next().is_none() {
        return taken;
    }

    if max_chars <= 3 {
        return ".".repeat(max_chars);
    }

    let visible: String = taken.chars().take(max_chars - 3).collect();
    format!("{visible}...")
}

fn pad_for_column(text: &str, width: usize) -> String {
    if width == 0 {
        return String::new();
    }

    let shown = truncate_for_column(text, width);
    let shown_len = shown.chars().count();
    if shown_len >= width {
        shown
    } else {
        format!("{shown}{}", " ".repeat(width - shown_len))
    }
}

fn render_diff_cell(cell: Option<&DiffCell>, width: usize) -> String {
    if width == 0 {
        return String::new();
    }

    let Some(cell) = cell else {
        return " ".repeat(width);
    };

    if cell.marker.is_none() && cell.line_number.is_none() {
        return pad_for_column(&cell.text, width);
    }

    let line_number = match cell.line_number {
        Some(n) => format!("{n:>width$}", width = DIFF_LINE_NUMBER_WIDTH),
        None => " ".repeat(DIFF_LINE_NUMBER_WIDTH),
    };
    let marker = cell.marker.unwrap_or(' ');
    let prefix = format!("{line_number} {marker} ");
    let prefix_width = prefix.chars().count();

    let combined = if width <= prefix_width {
        truncate_for_column(&prefix, width)
    } else {
        let content = truncate_for_column(&cell.text, width - prefix_width);
        format!("{prefix}{content}")
    };

    pad_for_column(&combined, width)
}

#[cfg(test)]
mod tests {
    use ratatui::{buffer::Buffer, layout::Rect, widgets::Widget};

    use super::{
        highlighted_segments, render_unified_diff, CodeDiff, CodeDiffBlock, CodeDiffLayout,
        CodeDiffLineKind, CodeDiffOptions,
    };

    #[test]
    fn classifies_file_hunks_and_changes() {
        let block = CodeDiffBlock {
            unified_diff: "--- a/a.rs\n+++ b/a.rs\n@@ -1 +1 @@\n-old\n+new\n context".to_string(),
        };
        let rendered = render_unified_diff(&block, &CodeDiffOptions::default());

        let kinds: Vec<CodeDiffLineKind> = rendered.lines.into_iter().map(|l| l.kind).collect();
        assert_eq!(
            kinds,
            vec![
                CodeDiffLineKind::Meta,
                CodeDiffLineKind::Meta,
                CodeDiffLineKind::Meta,
                CodeDiffLineKind::Remove,
                CodeDiffLineKind::Add,
                CodeDiffLineKind::Context,
            ]
        );
    }

    #[test]
    fn can_hide_file_headers() {
        let block = CodeDiffBlock {
            unified_diff: "--- a/a.rs\n+++ b/a.rs\n@@ -1 +1 @@\n-old\n+new".to_string(),
        };
        let options = CodeDiffOptions {
            show_file_headers: false,
            ..CodeDiffOptions::default()
        };
        let rendered = render_unified_diff(&block, &options);

        assert_eq!(rendered.lines.len(), 3);
        assert_eq!(rendered.lines[0].kind, CodeDiffLineKind::Meta);
        assert_eq!(rendered.lines[1].kind, CodeDiffLineKind::Remove);
        assert_eq!(rendered.lines[2].kind, CodeDiffLineKind::Add);
    }

    #[test]
    fn truncates_by_line_limit() {
        let block = CodeDiffBlock {
            unified_diff: "a\nb\nc\nd".to_string(),
        };
        let options = CodeDiffOptions {
            max_rendered_lines: 2,
            ..CodeDiffOptions::default()
        };

        let rendered = render_unified_diff(&block, &options);
        assert_eq!(rendered.lines.len(), 2);
        assert!(rendered.truncated);
    }

    #[test]
    fn malformed_input_falls_back_without_panic() {
        let block = CodeDiffBlock {
            unified_diff: "@@ bad\n+++\n---\n+\n-\n".to_string(),
        };
        let rendered = render_unified_diff(&block, &CodeDiffOptions::default());
        assert!(!rendered.lines.is_empty());
    }

    #[test]
    fn widget_renders_without_panic() {
        let widget = CodeDiff::from_unified_diff("--- a\n+++ b\n@@ -1 +1 @@\n-old\n+new\n");
        let area = Rect::new(0, 0, 40, 6);
        let mut buffer = Buffer::empty(area);

        (&widget).render(area, &mut buffer);

        assert!(buffer
            .content
            .iter()
            .any(|cell| !cell.symbol().trim().is_empty()));
    }

    #[test]
    fn side_by_side_layout_merges_changed_rows() {
        let widget = CodeDiff::from_unified_diff("@@ -1 +1 @@\n-old\n+new\n")
            .with_layout(CodeDiffLayout::SideBySide);
        assert_eq!(widget.measured_height(), 4);

        let area = Rect::new(0, 0, 60, 4);
        let mut buffer = Buffer::empty(area);
        (&widget).render(area, &mut buffer);

        let mut combined = String::new();
        for y in 0..area.height {
            for x in 0..area.width {
                combined.push_str(buffer[(x, y)].symbol());
            }
        }
        assert!(combined.contains("old"));
        assert!(combined.contains("new"));
    }

    #[test]
    fn side_by_side_hides_file_header_rows() {
        let widget = CodeDiff::from_unified_diff("--- a/src/lib.rs\n+++ b/src/lib.rs\n@@ -1 +1 @@\n-old\n+new\n")
            .with_layout(CodeDiffLayout::SideBySide);
        assert_eq!(widget.measured_height(), 4);

        let area = Rect::new(0, 0, 70, 4);
        let mut buffer = Buffer::empty(area);
        (&widget).render(area, &mut buffer);

        let mut first_row = String::new();
        for x in 0..area.width {
            first_row.push_str(buffer[(x, 1)].symbol());
        }
        assert!(first_row.contains("@@ -1 +1 @@"));
        assert!(!first_row.contains("--- a/src/lib.rs"));
        assert!(!first_row.contains("+++ b/src/lib.rs"));
    }

    #[test]
    fn side_by_side_does_not_duplicate_tokens_with_highlighting() {
        let widget = CodeDiff::from_unified_diff(
            "--- a/src/lib.rs\n+++ b/src/lib.rs\n@@ -1 +1 @@\n pub fn sum(a: i32, b: i32) -> i32 {\n",
        )
        .with_layout(CodeDiffLayout::SideBySide);

        let area = Rect::new(0, 0, 120, 4);
        let mut buffer = Buffer::empty(area);
        (&widget).render(area, &mut buffer);

        let mut row = String::new();
        for x in 0..area.width {
            row.push_str(buffer[(x, 2)].symbol());
        }

        assert!(!row.contains("pubpub"));
    }

    #[test]
    fn highlight_segments_preserve_source_text() {
        let text = "pub fn sum(a: i32, b: i32) -> i32 {";
        let segments = highlighted_segments(text, Some("rust")).expect("segments");
        let reconstructed = segments.into_iter().map(|(s, _)| s).collect::<String>();
        assert_eq!(reconstructed, text);
    }
}
