use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Rect},
    prelude::Stylize,
    style::{Color, Style},
    text::{Line, Span},
    widgets::Block,
};
use serde::Deserialize;
use serde_json::Value;
use std::iter::Peekable;

mod input;
mod messages;
mod overlays;
mod sidebar;
mod theme;

use super::app::{ChatApp, ChatMessage, SubagentStatusView};
use super::markdown::markdown_to_lines_with_indent;
use super::tool_presentation::render_tool_start;
use theme::*;
pub(crate) use theme::{AppLayoutRects, UiLayout};

#[derive(Debug, Deserialize)]
struct EditToolOutput {
    path: String,
    summary: EditDiffSummary,
    diff: String,
}

#[derive(Debug, Deserialize)]
struct EditDiffSummary {
    added_lines: usize,
    removed_lines: usize,
}

#[derive(Debug, Deserialize)]
struct TaskToolRenderOutput {
    name: String,
    agent_name: String,
    started_at: u64,
    #[serde(default)]
    finished_at: Option<u64>,
}

pub fn render_app(f: &mut Frame, app: &ChatApp) {
    let layout = UiLayout::default();
    f.render_widget(
        Block::default().style(Style::default().bg(PAGE_BG)),
        f.area(),
    );

    let app_area = inset_rect(
        f.area(),
        layout.main_outer_padding_x,
        layout.main_outer_padding_y,
    );
    let columns = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Min(40),
            Constraint::Length(layout.left_column_right_margin),
            Constraint::Length(layout.sidebar_width),
        ])
        .split(app_area);

    let main_area = columns[0];
    let sidebar_area = if columns.len() > 2 {
        Some(columns[2])
    } else {
        None
    };

    if app.is_viewing_subagent_session() {
        let main_chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Min(3),
                Constraint::Length(1),
                Constraint::Length(1),
            ])
            .split(main_area);

        render_messages(f, app, main_chunks[0]);
        render_subagent_back_indicator(f, app, main_chunks[2], layout);

        if let Some(area) = sidebar_area {
            let sidebar_bottom = main_chunks[2].bottom();
            let clipped_sidebar_area = Rect {
                x: area.x,
                y: area.y,
                width: area.width,
                height: sidebar_bottom.saturating_sub(area.y),
            };
            render_sidebar(f, app, clipped_sidebar_area);
        }

        render_clipboard_notice(f, app);
        return;
    }

    let input_content_width = main_area
        .width
        .saturating_sub(layout.user_bubble_indent() as u16 + 3)
        as usize;
    let input_line_count =
        input_line_count(&app.input, input_content_width).clamp(1, MAX_INPUT_LINES);
    let input_area_height = if app.has_pending_question() {
        (question_prompt_line_count(app, input_content_width) + 2) as u16
    } else {
        (input_line_count + 4) as u16
    };

    let main_chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Min(3),
            Constraint::Length(1),                 // Space above progress
            Constraint::Length(1),                 // Global processing indicator
            Constraint::Length(1),                 // Space above input
            Constraint::Length(input_area_height), // Input area
        ])
        .split(main_area);

    render_messages(f, app, main_chunks[0]);
    render_processing_indicator(f, app, main_chunks[2], layout);
    render_input(f, app, main_chunks[4], layout);

    if !app.filtered_commands.is_empty() {
        let item_count = app.filtered_commands.len().min(5) as u16;
        let popup_height = item_count;
        let input_left = main_chunks[4]
            .x
            .saturating_add(layout.user_bubble_indent() as u16);
        let input_width = main_chunks[4]
            .width
            .saturating_sub(layout.user_bubble_indent() as u16);
        let popup_area = Rect {
            x: input_left,
            y: main_chunks[4].y.saturating_sub(popup_height),
            width: input_width,
            height: popup_height,
        };
        render_command_palette(f, app, popup_area, layout);
    }

    if let Some(area) = sidebar_area {
        let sidebar_bottom = main_chunks[4].bottom();
        let clipped_sidebar_area = Rect {
            x: area.x,
            y: area.y,
            width: area.width,
            height: sidebar_bottom.saturating_sub(area.y),
        };
        render_sidebar(f, app, clipped_sidebar_area);
    }

    render_clipboard_notice(f, app);
}

fn render_subagent_back_indicator(f: &mut Frame, app: &ChatApp, area: Rect, layout: UiLayout) {
    let Some(view) = app.active_subagent_session() else {
        return;
    };

    let subagent_item = app
        .subagent_items
        .iter()
        .find(|item| item.task_id == view.task_id);
    let duration_secs = subagent_item
        .map(|item| {
            let end = item.finished_at.unwrap_or_else(now_unix_secs);
            end.saturating_sub(item.started_at)
        })
        .unwrap_or(0);

    let is_terminal = subagent_item.is_some_and(|item| item.status.is_terminal());
    if is_terminal {
        render_subagent_footer_line(f, app, area, layout, duration_secs, subagent_item);
        return;
    }

    let mut spans: Vec<Span<'static>> = vec![Span::raw(layout.message_indent())];
    let bar_len = area.width.saturating_sub(44).clamp(6, 10) as usize;
    let head = scanner_position(now_step(85), bar_len, 6);
    let base_color = app
        .selected_agent()
        .and_then(|agent| agent.color.as_ref())
        .and_then(|color_str| crate::agent::parse_color(color_str))
        .unwrap_or(PROGRESS_HEAD);

    for idx in 0..bar_len {
        let distance = head.abs_diff(idx);
        let (glyph, style) = if distance == 0 {
            (
                "■",
                Style::default()
                    .fg(base_color)
                    .add_modifier(ratatui::style::Modifier::BOLD),
            )
        } else if distance == 1 {
            (
                "■",
                Style::default().fg(blend_color_with_white(base_color, 0.30)),
            )
        } else if distance == 2 {
            (
                "■",
                Style::default().fg(blend_color_with_white(base_color, 0.40)),
            )
        } else {
            (
                "⬝",
                Style::default().fg(blend_color_with_white(base_color, 0.52)),
            )
        };
        spans.push(Span::styled(glyph, style));
    }

    spans.push(Span::raw(PROCESSING_STATUS_GAP));
    spans.push(Span::styled(
        format_elapsed_seconds(duration_secs),
        Style::default().fg(TEXT_MUTED),
    ));
    spans.push(Span::raw(PROCESSING_STATUS_GAP));
    spans.push(Span::styled("esc", Style::default().fg(TEXT_MUTED)));
    let back_label = if app.subagent_session_depth() > 1 {
        " back to upper subagent"
    } else {
        " back to main agent"
    };
    spans.push(Span::styled(back_label, Style::default().fg(TEXT_MUTED)));

    let paragraph =
        ratatui::widgets::Paragraph::new(Line::from(spans)).style(Style::default().bg(PAGE_BG));
    f.render_widget(paragraph, area);
}

fn render_subagent_footer_line(
    f: &mut Frame,
    app: &ChatApp,
    area: Rect,
    layout: UiLayout,
    duration_secs: u64,
    item: Option<&super::app::SubagentItemView>,
) {
    let agent = app.selected_agent();
    let agent_color = agent
        .and_then(|a| a.color.as_ref())
        .and_then(|c| crate::agent::parse_color(c))
        .unwrap_or(TEXT_PRIMARY);

    let provider_name = app
        .available_models
        .iter()
        .find(|model| model.full_id == app.selected_model_ref())
        .map(|model| model.provider_name.clone())
        .unwrap_or_default();
    let model_name = app
        .available_models
        .iter()
        .find(|model| model.full_id == app.selected_model_ref())
        .map(|model| model.model_name.clone())
        .unwrap_or_default();

    let is_failed = item.is_some_and(|row| {
        matches!(
            row.status,
            SubagentStatusView::Failed | SubagentStatusView::Cancelled
        )
    });
    let (status_symbol, status_color) = if is_failed {
        ("✗", Color::Red)
    } else {
        ("✓", Color::Rgb(25, 110, 61))
    };

    let mut spans = vec![
        Span::raw(layout.message_indent()),
        Span::styled(status_symbol, Style::default().fg(status_color)),
        Span::raw("  "),
        Span::styled(
            app.selected_agent()
                .map(|a| a.display_name.clone())
                .unwrap_or_else(|| "Agent".to_string()),
            Style::default().fg(agent_color),
        ),
        Span::raw("  "),
        Span::styled(provider_name, Style::default().fg(TEXT_MUTED)),
        Span::raw(" "),
        Span::styled(model_name, Style::default().fg(TEXT_MUTED)),
        Span::raw("  "),
        Span::styled(
            format_elapsed_seconds(duration_secs),
            Style::default().fg(TEXT_PRIMARY),
        ),
    ];

    if is_failed {
        spans.push(Span::raw("  "));
        spans.push(Span::styled("interrupted", Style::default().fg(Color::Red)));
    }

    let paragraph =
        ratatui::widgets::Paragraph::new(Line::from(spans)).style(Style::default().bg(PAGE_BG));
    f.render_widget(paragraph, area);
}

fn now_unix_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |duration| duration.as_secs())
}

fn now_step(interval_ms: u128) -> usize {
    let elapsed_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |duration| duration.as_millis());
    let interval = interval_ms.max(1);
    (elapsed_ms / interval) as usize
}

fn scanner_position(step: usize, width: usize, hold_frames: usize) -> usize {
    if width <= 1 {
        return 0;
    }

    let travel = width - 1;
    let cycle = hold_frames + travel + hold_frames + travel;
    let phase = step % cycle;

    if phase < hold_frames {
        0
    } else if phase < hold_frames + travel {
        phase - hold_frames
    } else if phase < hold_frames + travel + hold_frames {
        travel
    } else {
        travel - (phase - hold_frames - travel - hold_frames)
    }
}

fn blend_color_with_white(color: Color, amount: f64) -> Color {
    let amount = amount.clamp(0.0, 1.0);
    let to_rgb = match color {
        Color::Rgb(r, g, b) => Some((r, g, b)),
        Color::Black => Some((0, 0, 0)),
        Color::Red => Some((255, 0, 0)),
        Color::Green => Some((0, 200, 0)),
        Color::Yellow => Some((220, 180, 0)),
        Color::Blue => Some((0, 102, 255)),
        Color::Magenta => Some((200, 0, 200)),
        Color::Cyan => Some((0, 180, 200)),
        Color::White => Some((255, 255, 255)),
        Color::Gray | Color::DarkGray => Some((128, 128, 128)),
        Color::LightRed => Some((255, 110, 103)),
        Color::LightGreen => Some((105, 255, 105)),
        Color::LightYellow => Some((255, 255, 105)),
        Color::LightBlue => Some((98, 114, 164)),
        Color::LightMagenta => Some((246, 108, 181)),
        Color::LightCyan => Some((114, 159, 207)),
        Color::Indexed(_) | Color::Reset => None,
    };

    if let Some((r, g, b)) = to_rgb {
        Color::Rgb(
            (r as f64 + (255.0 - r as f64) * amount).round() as u8,
            (g as f64 + (255.0 - g as f64) * amount).round() as u8,
            (b as f64 + (255.0 - b as f64) * amount).round() as u8,
        )
    } else {
        color
    }
}

fn render_clipboard_notice(f: &mut Frame, app: &ChatApp) {
    overlays::render_clipboard_notice(f, app);
}

fn render_command_palette(f: &mut Frame, app: &ChatApp, area: Rect, layout: UiLayout) {
    overlays::render_command_palette(f, app, area, layout);
}

fn render_sidebar(f: &mut Frame, app: &ChatApp, area: Rect) {
    sidebar::render_sidebar(f, app, area);
}

pub(crate) fn build_sidebar_lines(app: &ChatApp, content_width: u16) -> Vec<Line<'static>> {
    sidebar::build_sidebar_lines(app, content_width)
}

fn render_messages(f: &mut Frame, app: &ChatApp, area: ratatui::layout::Rect) {
    messages::render_messages(f, app, area);
}

pub(crate) fn apply_selection_highlight(
    lines: &mut [Line<'static>],
    app: &ChatApp,
    line_offset: usize,
) {
    messages::apply_selection_highlight(lines, app, line_offset);
}

/// Build message lines (used for caching and scroll bounds)
pub fn build_message_lines(app: &ChatApp, width: usize) -> Vec<Line<'static>> {
    build_message_lines_with_starts(app, width).0
}

pub(crate) fn build_message_lines_with_starts(
    app: &ChatApp,
    width: usize,
) -> (Vec<Line<'static>>, Vec<usize>) {
    build_message_lines_impl(app, width, UiLayout::default())
}

pub(crate) fn append_message_lines_for_index(
    lines: &mut Vec<Line<'static>>,
    app: &ChatApp,
    width: usize,
    idx: usize,
) {
    let Some(msg) = app.messages.get(idx) else {
        return;
    };
    let layout = UiLayout::default();
    let message_indent = layout.message_indent();
    let tool_done_continuation = layout.message_child_indent();
    let tool_pending_prefix = format!("{message_indent}{TOOL_PENDING_MARKER}");
    let tool_pending_continuation = " ".repeat(tool_pending_prefix.chars().count());
    let tool_style = ToolCallRenderStyle {
        done_continuation: &tool_done_continuation,
        pending_prefix: &tool_pending_prefix,
        pending_continuation: &tool_pending_continuation,
    };
    let tool_context = ToolRenderContext {
        available_width: width.saturating_sub(4).max(1),
        style: tool_style,
        layout,
    };
    let border_color = if app.has_pending_question() {
        QUESTION_BORDER
    } else {
        app.selected_agent()
            .and_then(|agent| agent.color.as_ref())
            .and_then(|c| crate::agent::parse_color(c))
            .unwrap_or(ACCENT)
    };

    let render_context = MessageRenderContext {
        width,
        message_indent: &message_indent,
        tool_context,
        border_color,
    };

    render_message_line_item(lines, app, idx, msg, render_context);
}

fn build_message_lines_impl(
    app: &ChatApp,
    width: usize,
    layout: UiLayout,
) -> (Vec<Line<'static>>, Vec<usize>) {
    // Get agent color for user message borders
    let border_color = if app.has_pending_question() {
        QUESTION_BORDER
    } else {
        app.selected_agent()
            .and_then(|agent| agent.color.as_ref())
            .and_then(|c| crate::agent::parse_color(c))
            .unwrap_or(ACCENT)
    };
    let mut lines = Vec::new();
    let message_indent = layout.message_indent();
    let tool_done_continuation = layout.message_child_indent();
    let tool_pending_prefix = format!("{message_indent}{TOOL_PENDING_MARKER}");
    let tool_pending_continuation = " ".repeat(tool_pending_prefix.chars().count());
    let tool_style = ToolCallRenderStyle {
        done_continuation: &tool_done_continuation,
        pending_prefix: &tool_pending_prefix,
        pending_continuation: &tool_pending_continuation,
    };
    let tool_context = ToolRenderContext {
        available_width: width.saturating_sub(4).max(1),
        style: tool_style,
        layout,
    };
    let render_context = MessageRenderContext {
        width,
        message_indent: &message_indent,
        tool_context,
        border_color,
    };
    let mut message_starts = Vec::with_capacity(app.messages.len());

    for (idx, msg) in app.messages.iter().enumerate() {
        message_starts.push(lines.len());
        render_message_line_item(&mut lines, app, idx, msg, render_context);
    }

    (lines, message_starts)
}

#[derive(Clone, Copy)]
struct MessageRenderContext<'a> {
    width: usize,
    message_indent: &'a str,
    tool_context: ToolRenderContext<'a>,
    border_color: Color,
}

fn render_message_line_item(
    lines: &mut Vec<Line<'static>>,
    app: &ChatApp,
    idx: usize,
    msg: &ChatMessage,
    render_context: MessageRenderContext<'_>,
) {
    match msg {
        ChatMessage::User(text) => {
            render_user_message_block(
                lines,
                text,
                render_context.width,
                render_context.tool_context.layout,
                render_context.border_color,
            );
        }
        ChatMessage::Assistant(text) => {
            ensure_single_blank_line(lines);
            for line in
                parse_markdown_lines(text, render_context.width, render_context.message_indent)
            {
                lines.push(line);
            }
        }
        ChatMessage::CompactionPending => {
            render_compaction_block(
                lines,
                None,
                render_context.width,
                render_context.message_indent,
            );
        }
        ChatMessage::Compaction(summary) => {
            render_compaction_block(
                lines,
                Some(summary),
                render_context.width,
                render_context.message_indent,
            );
        }
        ChatMessage::Thinking(text) => {
            render_thinking_block(
                lines,
                text,
                render_context.width,
                render_context.message_indent,
            );
        }
        ChatMessage::ToolCall {
            name,
            args,
            output,
            is_error,
            ..
        } => {
            if idx > 0 && matches!(app.messages.get(idx - 1), Some(ChatMessage::Assistant(_))) {
                ensure_single_blank_line(lines);
            }
            render_tool_call_message(
                lines,
                ToolCallMessage {
                    name,
                    args,
                    output: output.as_deref(),
                    is_error: *is_error,
                },
                render_context.tool_context,
            );
        }
        ChatMessage::Error(text) => {
            lines.push(Line::from(""));
            lines.push(Line::from(vec![
                Span::raw(render_context.message_indent.to_string()),
                Span::styled("Error:", Style::default().fg(Color::Red).bold()),
                Span::raw(" "),
                Span::styled(text.clone(), Style::default().fg(Color::Red)),
            ]));
        }
        ChatMessage::Footer {
            agent_display_name,
            provider_name,
            model_name,
            duration,
            interrupted,
        } => {
            render_footer_block(
                lines,
                FooterBlock {
                    agent_display_name,
                    provider_name,
                    model_name,
                    duration,
                    interrupted: *interrupted,
                },
                render_context.message_indent,
                app.selected_agent(),
            );
        }
    }
}

/// Parse markdown text into styled lines with wrapping
fn parse_markdown_lines(text: &str, width: usize, indent: &str) -> Vec<Line<'static>> {
    markdown_to_lines_with_indent(text, width, indent)
}

fn parse_markdown_lines_unindented(text: &str, width: usize) -> Vec<Line<'static>> {
    markdown_to_lines_with_indent(text, width, "")
}

fn render_thinking_block(lines: &mut Vec<Line<'static>>, text: &str, width: usize, indent: &str) {
    ensure_single_blank_line(lines);

    let text = text.trim_end_matches(['\n', '\r']);

    let label = format!("{indent}Thinking: ");
    let label_width = label.chars().count();
    let wrapped = parse_markdown_lines_unindented(text, width.saturating_sub(label_width).max(1));

    if wrapped.is_empty() {
        lines.push(Line::from(Span::styled(
            label,
            Style::default().fg(THINKING_LABEL).italic(),
        )));
        lines.push(Line::from(""));
        return;
    }

    let continuation_indent = indent.to_string();
    for (index, line) in wrapped.into_iter().enumerate() {
        let mut spans = Vec::with_capacity(line.spans.len() + 1);
        if index == 0 {
            spans.push(Span::styled(
                label.clone(),
                Style::default().fg(THINKING_LABEL).italic(),
            ));
        } else {
            spans.push(Span::raw(continuation_indent.clone()));
        }

        spans.extend(line.spans.into_iter().map(|span| {
            let style = span.style.fg(TEXT_SECONDARY);
            Span::styled(span.content.into_owned(), style)
        }));

        lines.push(Line::from(spans));
    }

    lines.push(Line::from(""));
}

fn render_compaction_block(
    lines: &mut Vec<Line<'static>>,
    summary: Option<&str>,
    width: usize,
    indent: &str,
) {
    ensure_single_blank_line(lines);

    let label = " Compaction ";
    let available = width.saturating_sub(indent.chars().count());
    let total_rule = available.max(label.chars().count() + 4);
    let side = total_rule.saturating_sub(label.chars().count()) / 2;
    let left = "-".repeat(side);
    let right = "-".repeat(total_rule.saturating_sub(side + label.chars().count()));

    lines.push(Line::from(vec![
        Span::raw(indent.to_string()),
        Span::styled(left, Style::default().fg(TEXT_MUTED)),
        Span::styled(label, Style::default().fg(TEXT_MUTED)),
        Span::styled(right, Style::default().fg(TEXT_MUTED)),
    ]));
    lines.push(Line::from(""));

    if let Some(summary) = summary
        && !summary.trim().is_empty()
    {
        for line in parse_markdown_lines(summary, width, indent) {
            lines.push(line);
        }
    }
}

struct FooterBlock<'a> {
    agent_display_name: &'a str,
    provider_name: &'a str,
    model_name: &'a str,
    duration: &'a str,
    interrupted: bool,
}

fn render_footer_block(
    lines: &mut Vec<Line<'static>>,
    footer: FooterBlock<'_>,
    indent: &str,
    agent: Option<&super::app::AgentOptionView>,
) {
    // Get agent color, default to TEXT_PRIMARY
    let agent_color = agent
        .and_then(|a| a.color.as_ref())
        .and_then(|c| crate::agent::parse_color(c))
        .unwrap_or(TEXT_PRIMARY);

    // Checkmark or cross symbol with appropriate color
    let (status_symbol, status_color) = if footer.interrupted {
        ("✗", Color::Red)
    } else {
        ("✓", Color::Rgb(25, 110, 61))
    };

    // Build footer parts: symbol, agent name, provider, model, duration, interrupted
    let mut footer_parts: Vec<Span<'static>> = vec![
        Span::styled(status_symbol, Style::default().fg(status_color)),
        Span::raw("  "),
        Span::styled(
            footer.agent_display_name.to_string(),
            Style::default().fg(agent_color),
        ),
        Span::raw("  "),
        Span::styled(
            footer.provider_name.to_string(),
            Style::default().fg(TEXT_MUTED),
        ),
        Span::raw(" "),
        Span::styled(
            footer.model_name.to_string(),
            Style::default().fg(TEXT_MUTED),
        ),
        Span::raw("  "),
        Span::styled(
            footer.duration.to_string(),
            Style::default().fg(TEXT_PRIMARY),
        ),
    ];

    // Add "interrupted" text after duration if interrupted
    if footer.interrupted {
        footer_parts.push(Span::raw("  "));
        footer_parts.push(Span::styled("interrupted", Style::default().fg(Color::Red)));
    }

    // Add blank line before footer
    lines.push(Line::from(""));

    // Render footer line with message indentation
    let mut indent_spans = vec![Span::raw(indent.to_string())];
    indent_spans.extend(footer_parts);
    lines.push(Line::from(indent_spans));

    lines.push(Line::from(""));
}

/// Wrap text to a given width, returning a vector of lines.
fn wrap_text(text: &str, width: usize) -> Vec<String> {
    if width == 0 {
        return vec![text.to_string()];
    }

    let mut result = Vec::new();
    for line in text.lines() {
        if line.is_empty() {
            result.push(String::new());
            continue;
        }
        let mut current = String::new();
        for word in line.split_whitespace() {
            if current.is_empty() {
                current = word.to_string();
            } else if current.len() + 1 + word.len() <= width {
                current.push(' ');
                current.push_str(word);
            } else {
                result.push(current);
                current = word.to_string();
            }
        }
        if !current.is_empty() {
            result.push(current);
        }
    }
    if result.is_empty() {
        result.push(String::new());
    }
    result
}

fn wrap_compact_text(text: &str, width: usize) -> Vec<String> {
    if text.chars().count() > MAX_TOOL_OUTPUT_LEN {
        let truncated = truncate_chars(text, MAX_TOOL_OUTPUT_LEN);
        return wrap_text(&truncated, width);
    }
    wrap_text(text, width)
}

fn push_wrapped_tool_rows(
    lines: &mut Vec<Line<'static>>,
    wrapped: &[String],
    first_prefix: Vec<Span<'static>>,
    continuation_prefix: Vec<Span<'static>>,
    text_style: Style,
) {
    for (index, text) in wrapped.iter().enumerate() {
        let mut row = if index == 0 {
            first_prefix.clone()
        } else {
            continuation_prefix.clone()
        };
        row.push(Span::styled(text.clone(), text_style));
        lines.push(Line::from(row));
    }
}

#[derive(Clone, Copy)]
struct ToolCallRenderStyle<'a> {
    done_continuation: &'a str,
    pending_prefix: &'a str,
    pending_continuation: &'a str,
}

#[derive(Clone, Copy)]
struct ToolRenderContext<'a> {
    available_width: usize,
    style: ToolCallRenderStyle<'a>,
    layout: UiLayout,
}

#[derive(Clone, Copy)]
struct ToolCallMessage<'a> {
    name: &'a str,
    args: &'a str,
    output: Option<&'a str>,
    is_error: Option<bool>,
}

#[derive(Clone, Copy)]
struct CompletedToolCall<'a> {
    name: &'a str,
    label: &'a str,
    output: Option<&'a str>,
    is_error: bool,
}

fn render_tool_call_message(
    lines: &mut Vec<Line<'static>>,
    message: ToolCallMessage<'_>,
    context: ToolRenderContext<'_>,
) {
    let args_value: Value = serde_json::from_str(message.args).unwrap_or(Value::Null);
    let label = render_tool_start(message.name, &args_value).line;

    match message.is_error {
        Some(error) => {
            if !error
                && (message.name == "edit" || message.name == "write")
                && let Some(tool_output) = message.output
                && render_edit_diff_block(
                    lines,
                    message.name,
                    tool_output,
                    context.available_width,
                    context.layout,
                )
            {
                return;
            }

            render_completed_tool_call(
                lines,
                CompletedToolCall {
                    name: message.name,
                    label: &label,
                    output: message.output,
                    is_error: error,
                },
                context,
            );
        }
        None => render_pending_tool_call(
            lines,
            message.name,
            &label,
            message.args,
            context.available_width,
            context.style.pending_prefix,
            context.style.pending_continuation,
        ),
    }
}

fn render_completed_tool_call(
    lines: &mut Vec<Line<'static>>,
    completed: CompletedToolCall<'_>,
    context: ToolRenderContext<'_>,
) {
    let completed_label = if completed.name == "task" {
        task_completed_label(completed.label, completed.output)
    } else if completed.is_error {
        completed.label.to_string()
    } else {
        append_tool_result_count(completed.name, completed.label, completed.output)
    };
    let symbol = if completed.is_error { "✗" } else { "✓" };
    let color = if completed.is_error {
        Color::Red
    } else {
        INPUT_ACCENT
    };
    let wrapped = wrap_compact_text(&completed_label, context.available_width);

    push_wrapped_tool_rows(
        lines,
        &wrapped,
        vec![
            Span::raw(context.layout.message_indent()),
            Span::styled(symbol, Style::default().fg(color).bold()),
            Span::raw(" "),
        ],
        vec![Span::raw(context.style.done_continuation.to_string())],
        Style::default().fg(TEXT_SECONDARY),
    );

    if completed.is_error {
        render_tool_error_detail(lines, completed.output, context);
    }
}

fn render_tool_error_detail(
    lines: &mut Vec<Line<'static>>,
    output: Option<&str>,
    context: ToolRenderContext<'_>,
) {
    let Some(error_text) = extract_tool_error_text(output) else {
        return;
    };

    let wrapped = wrap_compact_text(
        &error_text,
        context.available_width.saturating_sub(2).max(1),
    );

    push_wrapped_tool_rows(
        lines,
        &wrapped,
        vec![
            Span::raw(context.layout.message_child_indent()),
            Span::styled("└ ", Style::default().fg(Color::Red)),
        ],
        vec![
            Span::raw(context.layout.message_child_indent()),
            Span::raw("  "),
        ],
        Style::default().fg(Color::Red),
    );
}

fn extract_tool_error_text(output: Option<&str>) -> Option<String> {
    let trimmed = output?.trim();
    if trimmed.is_empty() {
        return None;
    }

    let Ok(parsed) = serde_json::from_str::<Value>(trimmed) else {
        return Some(trimmed.to_string());
    };

    extract_error_message_from_json(&parsed)
        .or_else(|| serde_json::to_string_pretty(&parsed).ok())
        .filter(|text| !text.trim().is_empty())
}

fn extract_error_message_from_json(value: &Value) -> Option<String> {
    match value {
        Value::String(text) => {
            let trimmed = text.trim();
            if trimmed.is_empty() {
                None
            } else {
                Some(trimmed.to_string())
            }
        }
        Value::Array(items) => items.iter().find_map(extract_error_message_from_json),
        Value::Object(map) => {
            const PRIORITY_KEYS: [&str; 7] = [
                "error", "message", "stderr", "details", "detail", "reason", "summary",
            ];

            for key in PRIORITY_KEYS {
                if let Some(value) = map.get(key)
                    && let Some(message) = extract_error_message_from_json(value)
                {
                    return Some(message);
                }
            }

            map.values().find_map(extract_error_message_from_json)
        }
        _ => {
            let as_text = value.to_string();
            if as_text.trim().is_empty() {
                None
            } else {
                Some(as_text)
            }
        }
    }
}

fn render_pending_tool_call(
    lines: &mut Vec<Line<'static>>,
    tool_name: &str,
    label: &str,
    args: &str,
    available_width: usize,
    tool_pending_prefix: &str,
    tool_pending_continuation: &str,
) {
    let pending_label = if tool_name == "task" {
        let elapsed = task_pending_elapsed_secs(args).unwrap_or(0);
        format!(
            "{label}  {}  (click to open)",
            format_elapsed_seconds(elapsed)
        )
    } else {
        label.to_string()
    };
    let wrapped = wrap_compact_text(&pending_label, available_width.saturating_sub(1));
    push_wrapped_tool_rows(
        lines,
        &wrapped,
        vec![Span::styled(
            tool_pending_prefix.to_string(),
            Style::default().fg(TEXT_MUTED),
        )],
        vec![Span::raw(tool_pending_continuation.to_string())],
        Style::default().fg(TEXT_SECONDARY),
    );
}

fn task_pending_elapsed_secs(args: &str) -> Option<u64> {
    let args_value = serde_json::from_str::<Value>(args).ok()?;
    let started_at = args_value
        .as_object()
        .and_then(|map| map.get("__started_at"))
        .and_then(Value::as_u64)?;
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .ok()?
        .as_secs();
    Some(now.saturating_sub(started_at))
}

fn task_completed_label(base_label: &str, output: Option<&str>) -> String {
    let Some(output) = output else {
        return format!("{base_label}  0s");
    };

    let Ok(parsed) = serde_json::from_str::<TaskToolRenderOutput>(output) else {
        return format!("{base_label}  0s");
    };

    let label = format!("Task [{}]: {}", title_case(&parsed.agent_name), parsed.name);
    let finished = parsed.finished_at.unwrap_or(parsed.started_at);
    format!(
        "{}  {}  (click to open)",
        label,
        format_elapsed_seconds(finished.saturating_sub(parsed.started_at))
    )
}

fn format_elapsed_seconds(secs: u64) -> String {
    if secs < 60 {
        return format!("{}s", secs);
    }
    let mins = secs / 60;
    let rem = secs % 60;
    format!("{}m {}s", mins, rem)
}

fn title_case(name: &str) -> String {
    let mut result = String::new();
    let mut capitalize = true;
    for ch in name.chars() {
        if matches!(ch, '_' | '-' | ' ') {
            if !result.ends_with(' ') {
                result.push(' ');
            }
            capitalize = true;
            continue;
        }
        if capitalize {
            result.extend(ch.to_uppercase());
            capitalize = false;
        } else {
            result.extend(ch.to_lowercase());
        }
    }
    result.trim().to_string()
}

fn render_input(f: &mut Frame, app: &ChatApp, area: Rect, layout: UiLayout) {
    input::render_input(f, app, area, layout);
}

fn question_prompt_line_count(app: &ChatApp, _width: usize) -> usize {
    input::question_prompt_line_count(app, _width)
}

fn input_line_count(input: &str, width: usize) -> usize {
    input::input_line_count(input, width)
}

fn render_processing_indicator(f: &mut Frame, app: &ChatApp, area: Rect, layout: UiLayout) {
    input::render_processing_indicator(f, app, area, layout);
}

fn inset_rect(area: Rect, padding_x: u16, padding_y: u16) -> Rect {
    Rect {
        x: area.x.saturating_add(padding_x),
        y: area.y.saturating_add(padding_y),
        width: area.width.saturating_sub(padding_x.saturating_mul(2)),
        height: area.height.saturating_sub(padding_y.saturating_mul(2)),
    }
}

pub(crate) fn compute_layout_rects(area: Rect, app: &ChatApp) -> AppLayoutRects {
    let layout = UiLayout::default();
    let app_area = inset_rect(
        area,
        layout.main_outer_padding_x,
        layout.main_outer_padding_y,
    );
    let columns = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Min(40),
            Constraint::Length(layout.left_column_right_margin),
            Constraint::Length(layout.sidebar_width),
        ])
        .split(app_area);

    let main_area = columns[0];
    let sidebar_area = if columns.len() > 2 {
        Some(columns[2])
    } else {
        None
    };

    let input_content_width = main_area
        .width
        .saturating_sub(layout.user_bubble_indent() as u16 + 3)
        as usize;
    let input_line_count =
        input_line_count(&app.input, input_content_width).clamp(1, MAX_INPUT_LINES);
    let input_area_height = if app.has_pending_question() {
        (question_prompt_line_count(app, input_content_width) + 2) as u16
    } else {
        (input_line_count + 4) as u16
    };
    let main_chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Min(3),
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Length(input_area_height),
        ])
        .split(main_area);

    let sidebar_content = sidebar_area.and_then(|sidebar_area| {
        let sidebar_bottom = main_chunks[4].bottom();
        let clipped_sidebar_area = Rect {
            x: sidebar_area.x,
            y: sidebar_area.y,
            width: sidebar_area.width,
            height: sidebar_bottom.saturating_sub(sidebar_area.y),
        };
        if clipped_sidebar_area.width == 0 || clipped_sidebar_area.height == 0 {
            return None;
        }

        let block = Block::default().style(Style::default().bg(SIDEBAR_BG));
        let inner = block.inner(clipped_sidebar_area);
        let content = inset_rect(inner, 2, 0);
        if content.width == 0 || content.height == 0 {
            None
        } else {
            Some(content)
        }
    });

    let main_messages = if main_chunks[0].height > 0 {
        Some(main_chunks[0])
    } else {
        None
    };

    AppLayoutRects {
        main_messages,
        sidebar_content,
    }
}

fn render_edit_diff_block(
    lines: &mut Vec<Line<'static>>,
    tool_name: &str,
    output: &str,
    available_width: usize,
    layout: UiLayout,
) -> bool {
    let parsed: EditToolOutput = match serde_json::from_str(output) {
        Ok(value) => value,
        Err(_) => return false,
    };
    let child_indent = layout.message_child_indent();

    lines.push(Line::from(vec![
        Span::raw(layout.message_indent()),
        Span::styled("✓ ", Style::default().fg(INPUT_ACCENT).bold()),
        Span::styled(
            format!(
                "{} {}  +{} -{}",
                tool_title(tool_name),
                parsed.path,
                parsed.summary.added_lines,
                parsed.summary.removed_lines
            ),
            Style::default().fg(TEXT_SECONDARY),
        ),
    ]));

    let (left_width, right_width) = diff_column_widths(available_width);
    if left_width < MIN_DIFF_COLUMN_WIDTH || right_width < MIN_DIFF_COLUMN_WIDTH {
        return render_edit_diff_block_single_column(lines, &parsed.diff, available_width, layout);
    }

    let mut rendered_chars = 0;
    let mut truncated = false;

    let mut raw_lines = parsed.diff.lines().peekable();
    let mut cursor = DiffLineCursor::default();
    let mut rendered_lines = 0;
    while let Some(side_by_side) = next_diff_row(&mut raw_lines, &mut cursor) {
        let line_chars = side_by_side.total_chars();
        if rendered_lines >= MAX_RENDERED_DIFF_LINES
            || rendered_chars + line_chars > MAX_RENDERED_DIFF_CHARS
        {
            truncated = true;
            break;
        }
        rendered_chars += line_chars;
        rendered_lines += 1;

        render_side_by_side_diff_row(lines, &side_by_side, left_width, right_width, layout);
    }

    if truncated {
        lines.push(Line::from(vec![
            Span::raw(child_indent.clone()),
            Span::styled(
                "... diff truncated",
                Style::default().fg(TEXT_MUTED).italic(),
            ),
        ]));
    }

    true
}

fn render_edit_diff_block_single_column(
    lines: &mut Vec<Line<'static>>,
    diff: &str,
    available_width: usize,
    layout: UiLayout,
) -> bool {
    let mut rendered_chars = 0;
    let mut truncated = false;
    let child_indent = layout.message_child_indent();

    for (rendered_lines, raw_line) in diff.lines().enumerate() {
        let line_chars = raw_line.chars().count();
        if rendered_lines >= MAX_RENDERED_DIFF_LINES
            || rendered_chars + line_chars > MAX_RENDERED_DIFF_CHARS
        {
            truncated = true;
            break;
        }
        rendered_chars += line_chars;

        let shown = truncate_chars(raw_line, available_width);
        let style = if raw_line.starts_with('+') && !raw_line.starts_with("+++") {
            Style::default().fg(DIFF_ADD_FG).bg(DIFF_ADD_BG)
        } else if raw_line.starts_with('-') && !raw_line.starts_with("---") {
            Style::default().fg(DIFF_REMOVE_FG).bg(DIFF_REMOVE_BG)
        } else if raw_line.starts_with("@@")
            || raw_line.starts_with("---")
            || raw_line.starts_with("+++")
        {
            Style::default().fg(DIFF_META_FG)
        } else {
            Style::default().fg(TEXT_MUTED)
        };

        lines.push(Line::from(vec![
            Span::raw(child_indent.clone()),
            Span::styled(shown, style),
        ]));
    }

    if truncated {
        lines.push(Line::from(vec![
            Span::raw(child_indent.clone()),
            Span::styled(
                "... diff truncated",
                Style::default().fg(TEXT_MUTED).italic(),
            ),
        ]));
    }

    true
}

fn render_user_message_block(
    lines: &mut Vec<Line<'static>>,
    text: &str,
    width: usize,
    layout: UiLayout,
    border_color: Color,
) {
    let content_width = width.saturating_sub(layout.user_bubble_indent() + 1).max(1);
    let text_width = content_width
        .saturating_sub(layout.user_bubble_inner_padding * 2)
        .max(1);
    let wrapped = wrap_text(text, text_width);

    ensure_single_blank_line(lines);
    lines.push(build_user_bubble_line(
        "",
        content_width,
        layout,
        border_color,
    ));
    for line in wrapped {
        lines.push(build_user_bubble_line(
            &line,
            content_width,
            layout,
            border_color,
        ));
    }
    lines.push(build_user_bubble_line(
        "",
        content_width,
        layout,
        border_color,
    ));
    lines.push(Line::from(""));
}

fn ensure_single_blank_line(lines: &mut Vec<Line<'static>>) {
    if lines.is_empty() {
        return;
    }
    if let Some(last) = lines.last()
        && line_is_empty(last)
    {
        return;
    }
    lines.push(Line::from(""));
}

fn line_is_empty(line: &Line<'_>) -> bool {
    line.spans.iter().all(|span| span.content.is_empty())
}

fn build_user_bubble_line(
    content: &str,
    content_width: usize,
    layout: UiLayout,
    border_color: Color,
) -> Line<'static> {
    let trimmed = truncate_chars(
        content,
        content_width.saturating_sub(layout.user_bubble_inner_padding * 2),
    );
    let leading = " ".repeat(layout.user_bubble_inner_padding);
    let trailing_len = content_width
        .saturating_sub(layout.user_bubble_inner_padding * 2)
        .saturating_sub(trimmed.chars().count());
    let trailing = " ".repeat(trailing_len + layout.user_bubble_inner_padding);

    Line::from(vec![
        Span::raw(" ".repeat(layout.user_bubble_indent())),
        Span::styled("▌", Style::default().fg(border_color).bg(INPUT_PANEL_BG)),
        Span::styled(
            format!("{}{}{}", leading, trimmed, trailing),
            Style::default().fg(TEXT_PRIMARY).bg(INPUT_PANEL_BG),
        ),
    ])
}

fn append_tool_result_count(name: &str, label: &str, output: Option<&str>) -> String {
    let Some(raw_output) = output else {
        return label.to_string();
    };
    let Ok(value) = serde_json::from_str::<Value>(raw_output) else {
        return label.to_string();
    };
    let Some(count) = value.get("count").and_then(|v| v.as_u64()) else {
        return label.to_string();
    };

    match name {
        "list" => format!("{label} ({count} entries)"),
        "glob" => format!("{label} ({count} files)"),
        "grep" => format!("{label} ({count} matches)"),
        _ => label.to_string(),
    }
}

fn diff_column_widths(available_width: usize) -> (usize, usize) {
    let inner_width = available_width.saturating_sub(7);
    let left = inner_width / 2;
    let right = inner_width.saturating_sub(left);
    (left, right)
}

#[derive(Debug)]
struct SideBySideDiffRow {
    left: Option<DiffCell>,
    right: Option<DiffCell>,
    kind: SideBySideDiffKind,
}

impl SideBySideDiffRow {
    fn total_chars(&self) -> usize {
        self.left
            .as_ref()
            .map(|cell| cell.text.chars().count())
            .unwrap_or(0)
            + self
                .right
                .as_ref()
                .map(|cell| cell.text.chars().count())
                .unwrap_or(0)
    }
}

#[derive(Debug, Clone)]
struct DiffCell {
    line_number: Option<usize>,
    marker: Option<char>,
    text: String,
}

#[derive(Debug, Default)]
struct DiffLineCursor {
    left_line: Option<usize>,
    right_line: Option<usize>,
}

#[derive(Debug, Clone, Copy)]
enum SideBySideDiffKind {
    Context,
    Removed,
    Added,
    Meta,
    Changed,
}

fn next_diff_row<'a>(
    lines: &mut Peekable<impl Iterator<Item = &'a str>>,
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

    if let Some(context_text) = raw.strip_prefix(' ') {
        return Some(SideBySideDiffRow {
            left: Some(DiffCell {
                line_number: take_next_line_number(&mut cursor.left_line),
                marker: None,
                text: context_text.to_string(),
            }),
            right: Some(DiffCell {
                line_number: take_next_line_number(&mut cursor.right_line),
                marker: None,
                text: context_text.to_string(),
            }),
            kind: SideBySideDiffKind::Context,
        });
    }

    if raw.starts_with('-') && !raw.starts_with("---") {
        if let Some(next) = lines.peek()
            && next.starts_with('+')
            && !next.starts_with("+++")
        {
            let added = lines.next().unwrap_or_default().to_string();
            let removed_text = raw.strip_prefix('-').unwrap_or(raw);
            let added_text = added.strip_prefix('+').unwrap_or(&added);
            return Some(SideBySideDiffRow {
                left: Some(DiffCell {
                    line_number: take_next_line_number(&mut cursor.left_line),
                    marker: Some('-'),
                    text: removed_text.to_string(),
                }),
                right: Some(DiffCell {
                    line_number: take_next_line_number(&mut cursor.right_line),
                    marker: Some('+'),
                    text: added_text.to_string(),
                }),
                kind: SideBySideDiffKind::Changed,
            });
        }

        let removed_text = raw.strip_prefix('-').unwrap_or(raw);

        return Some(SideBySideDiffRow {
            left: Some(DiffCell {
                line_number: take_next_line_number(&mut cursor.left_line),
                marker: Some('-'),
                text: removed_text.to_string(),
            }),
            right: None,
            kind: SideBySideDiffKind::Removed,
        });
    }

    if raw.starts_with('+') && !raw.starts_with("+++") {
        let added_text = raw.strip_prefix('+').unwrap_or(raw);
        return Some(SideBySideDiffRow {
            left: None,
            right: Some(DiffCell {
                line_number: take_next_line_number(&mut cursor.right_line),
                marker: Some('+'),
                text: added_text.to_string(),
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

fn render_side_by_side_diff_row(
    lines: &mut Vec<Line<'static>>,
    row: &SideBySideDiffRow,
    left_width: usize,
    right_width: usize,
    layout: UiLayout,
) {
    let left_text = render_diff_cell(row.left.as_ref(), left_width);
    let right_text = render_diff_cell(row.right.as_ref(), right_width);

    let (left_style, right_style) = match row.kind {
        SideBySideDiffKind::Context => (
            Style::default().fg(TEXT_MUTED),
            Style::default().fg(TEXT_MUTED),
        ),
        SideBySideDiffKind::Removed => (
            Style::default().fg(DIFF_REMOVE_FG).bg(DIFF_REMOVE_BG),
            Style::default().fg(TEXT_MUTED),
        ),
        SideBySideDiffKind::Added => (
            Style::default().fg(TEXT_MUTED),
            Style::default().fg(DIFF_ADD_FG).bg(DIFF_ADD_BG),
        ),
        SideBySideDiffKind::Meta => (
            Style::default().fg(DIFF_META_FG),
            Style::default().fg(DIFF_META_FG),
        ),
        SideBySideDiffKind::Changed => (
            Style::default().fg(DIFF_REMOVE_FG).bg(DIFF_REMOVE_BG),
            Style::default().fg(DIFF_ADD_FG).bg(DIFF_ADD_BG),
        ),
    };

    lines.push(Line::from(vec![
        Span::raw(layout.message_child_indent()),
        Span::styled(left_text, left_style),
        Span::styled(" | ", Style::default().fg(DIFF_META_FG)),
        Span::styled(right_text, right_style),
    ]));
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

fn truncate_for_column(input: &str, max_chars: usize) -> String {
    truncate_chars_impl(input, max_chars, TruncationMode::FixedWidth)
}

fn tool_title(name: &str) -> &'static str {
    match name {
        "edit" => "Edit",
        "write" => "Write",
        _ => "Tool",
    }
}

fn truncate_chars(input: &str, max_chars: usize) -> String {
    truncate_chars_impl(input, max_chars, TruncationMode::AppendEllipsis)
}

#[derive(Clone, Copy)]
enum TruncationMode {
    FixedWidth,
    AppendEllipsis,
}

fn truncate_chars_impl(input: &str, max_chars: usize, mode: TruncationMode) -> String {
    if max_chars == 0 {
        return String::new();
    }

    let mut chars = input.chars();
    let taken: String = chars.by_ref().take(max_chars).collect();
    if chars.next().is_none() {
        return taken;
    }

    match mode {
        TruncationMode::FixedWidth => {
            if max_chars <= 3 {
                ".".repeat(max_chars)
            } else {
                let visible: String = taken.chars().take(max_chars - 3).collect();
                format!("{visible}...")
            }
        }
        TruncationMode::AppendEllipsis => format!("{taken}..."),
    }
}
