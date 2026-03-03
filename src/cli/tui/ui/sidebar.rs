use ratatui::{
    Frame,
    prelude::Stylize,
    style::Style,
    text::{Line, Span, Text},
    widgets::{Block, Paragraph, Wrap},
};
use serde_json::Value;

use super::super::app::{ChatApp, ChatMessage, TodoItemView, TodoStatus};
use super::theme::*;

pub(super) fn render_sidebar(f: &mut Frame, app: &ChatApp, area: ratatui::layout::Rect) {
    let block = Block::default().style(Style::default().bg(SIDEBAR_BG));
    let inner = block.inner(area);
    let content = super::inset_rect(inner, 2, 0);
    f.render_widget(block, area);

    let lines = build_sidebar_lines(app, content.width);
    let scroll_offset = app
        .sidebar_scroll
        .effective_offset(lines.len(), content.height as usize);

    let sidebar = Paragraph::new(Text::from(lines.to_vec()))
        .style(Style::default().bg(SIDEBAR_BG))
        .wrap(Wrap { trim: true })
        .scroll((scroll_offset as u16, 0));
    f.render_widget(sidebar, content);
}

pub(super) fn build_sidebar_lines(app: &ChatApp, content_width: u16) -> Vec<Line<'static>> {
    let content_width = content_width.max(1);

    let (used, budget) = app.context_usage();
    let context_percent = if budget == 0 {
        0
    } else {
        (used.saturating_mul(100) / budget).min(999)
    };
    let context_usage_color = if context_percent >= 60 {
        CONTEXT_USAGE_RED
    } else if context_percent >= 40 {
        CONTEXT_USAGE_ORANGE
    } else if context_percent >= 30 {
        CONTEXT_USAGE_YELLOW
    } else {
        TEXT_PRIMARY
    };

    let directory_text =
        format_sidebar_directory(&app.working_directory, app.git_branch.as_deref());
    let mut lines: Vec<Line<'static>> = vec![
        Line::from(""),
        Line::from(Span::styled(
            sidebar_prefixed(&app.session_name),
            Style::default().fg(TEXT_PRIMARY).bold(),
        )),
        Line::from(""),
        Line::from(Span::styled(
            sidebar_prefixed(&abbreviate_path(
                &directory_text,
                content_width.saturating_sub(2) as usize,
            )),
            Style::default().fg(TEXT_PRIMARY),
        )),
        Line::from(""),
    ];

    let mut sections: Vec<Vec<Line<'static>>> = Vec::new();
    sections.push(vec![
        Line::from(Span::styled(
            sidebar_label("Context"),
            Style::default().fg(TEXT_SECONDARY).bold(),
        )),
        Line::from(Span::styled(
            sidebar_prefixed(&format!("{} / {} ({}%)", used, budget, context_percent)),
            Style::default().fg(context_usage_color),
        )),
    ]);

    let modified_files = collect_modified_files(&app.messages);
    if !modified_files.is_empty() {
        let mut modified_lines = vec![Line::from(Span::styled(
            sidebar_label("Modified Files"),
            Style::default().fg(TEXT_SECONDARY).bold(),
        ))];
        append_modified_file_list(&mut modified_lines, &modified_files, content_width as usize);
        sections.push(modified_lines);
    }

    if !app.todo_items.is_empty() {
        let mut todo_lines = vec![Line::from(Span::styled(
            sidebar_label("TODO"),
            Style::default().fg(TEXT_SECONDARY).bold(),
        ))];
        let done = app
            .todo_items
            .iter()
            .filter(|item| item.status == TodoStatus::Completed)
            .count();
        todo_lines.push(Line::from(Span::styled(
            sidebar_label(&format!("{} / {} done", done, app.todo_items.len())),
            Style::default().fg(TEXT_MUTED),
        )));

        append_sidebar_list(&mut todo_lines, &app.todo_items, app.todo_items.len());
        sections.push(todo_lines);
    }

    let section_count = sections.len();
    for (index, section) in sections.into_iter().enumerate() {
        lines.extend(section);
        if index + 1 < section_count {
            lines.push(Line::from(""));
        }
    }

    lines
}

fn abbreviate_path(path: &str, max_chars: usize) -> String {
    if max_chars == 0 {
        return String::new();
    }
    let path_chars = path.chars().count();
    if path_chars <= max_chars {
        return path.to_string();
    }

    let tail_chars = max_chars.saturating_sub(3);
    let tail: String = path
        .chars()
        .rev()
        .take(tail_chars)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect();
    format!("...{}", tail)
}

fn format_sidebar_directory(path: &str, git_branch: Option<&str>) -> String {
    let simplified = simplify_home_path(path);
    match git_branch {
        Some(branch) if !branch.is_empty() => format!("{simplified} @ {branch}"),
        _ => simplified,
    }
}

fn simplify_home_path(path: &str) -> String {
    let Some(home) = dirs::home_dir() else {
        return path.to_string();
    };

    let home = home.to_string_lossy();
    if path == home {
        return "~".to_string();
    }

    let home_prefix = format!("{home}/");
    if let Some(rest) = path.strip_prefix(&home_prefix) {
        return format!("~/{rest}");
    }

    path.to_string()
}

#[derive(Debug, Clone)]
struct ModifiedFileSummary {
    path: String,
    added_lines: usize,
    removed_lines: usize,
}

fn collect_modified_files(messages: &[ChatMessage]) -> Vec<ModifiedFileSummary> {
    let mut files: Vec<ModifiedFileSummary> = Vec::new();

    for message in messages {
        let ChatMessage::ToolCall {
            output, is_error, ..
        } = message
        else {
            continue;
        };

        if !matches!(is_error, Some(false)) {
            continue;
        }

        let Some(output) = output else {
            continue;
        };

        let Some(parsed) = parse_modified_file_summary(output) else {
            continue;
        };

        if let Some(existing) = files.iter_mut().find(|item| item.path == parsed.path) {
            existing.added_lines = existing.added_lines.saturating_add(parsed.added_lines);
            existing.removed_lines = existing.removed_lines.saturating_add(parsed.removed_lines);
            continue;
        }

        files.push(parsed);
    }

    files
}

fn parse_modified_file_summary(output: &str) -> Option<ModifiedFileSummary> {
    let value = serde_json::from_str::<Value>(output).ok()?;
    let path = value.get("path")?.as_str()?.to_string();
    let summary = value.get("summary")?;
    let added_lines = summary.get("added_lines")?.as_u64()? as usize;
    let removed_lines = summary.get("removed_lines")?.as_u64()? as usize;

    if added_lines == 0 && removed_lines == 0 {
        return None;
    }

    Some(ModifiedFileSummary {
        path,
        added_lines,
        removed_lines,
    })
}

fn append_modified_file_list(
    lines: &mut Vec<Line<'static>>,
    files: &[ModifiedFileSummary],
    content_width: usize,
) {
    let line_width = content_width.saturating_sub(SIDEBAR_INDENT.chars().count());

    for file in files {
        let added_text = if file.added_lines > 0 {
            format!("+{}", file.added_lines)
        } else {
            String::new()
        };
        let removed_text = if file.removed_lines > 0 {
            format!("-{}", file.removed_lines)
        } else {
            String::new()
        };
        let has_added = !added_text.is_empty();

        let gap = if has_added && !removed_text.is_empty() {
            1
        } else {
            0
        };
        let delta_len = added_text.chars().count() + removed_text.chars().count() + gap;
        let path_max = line_width.saturating_sub(delta_len + 1);
        let path_text = super::truncate_chars(&file.path, path_max.max(1));
        let spaces = line_width
            .saturating_sub(path_text.chars().count() + delta_len)
            .max(1);

        let mut spans = vec![
            Span::styled(
                sidebar_prefixed(&path_text),
                Style::default().fg(TEXT_SECONDARY),
            ),
            Span::raw(" ".repeat(spaces)),
        ];

        if has_added {
            spans.push(Span::styled(
                added_text,
                Style::default().fg(DIFF_ADD_FG).bold(),
            ));
        }
        if !removed_text.is_empty() {
            if has_added {
                spans.push(Span::raw(" "));
            }
            spans.push(Span::styled(
                removed_text,
                Style::default().fg(DIFF_REMOVE_FG).bold(),
            ));
        }

        lines.push(Line::from(spans));
    }
}

fn append_sidebar_list(lines: &mut Vec<Line<'static>>, items: &[TodoItemView], max_items: usize) {
    if max_items == 0 {
        return;
    }
    if items.is_empty() {
        lines.push(Line::from(Span::styled(
            sidebar_prefixed("none"),
            Style::default().fg(TEXT_MUTED),
        )));
        return;
    }

    let shown = items.len().min(max_items);
    for item in items.iter().take(shown) {
        let (marker, item_style) = match item.status {
            TodoStatus::Pending | TodoStatus::InProgress => {
                ("[ ] ", Style::default().fg(TEXT_PRIMARY))
            }
            TodoStatus::Completed => ("[x] ", Style::default().fg(TEXT_MUTED)),
            TodoStatus::Cancelled => ("[-] ", Style::default().fg(TEXT_MUTED)),
        };

        lines.push(Line::from(vec![
            Span::styled(sidebar_prefixed(marker), Style::default().fg(INPUT_ACCENT)),
            Span::styled(item.content.clone(), item_style),
        ]));
    }

    if items.len() > shown {
        lines.push(Line::from(Span::styled(
            "...",
            Style::default().fg(TEXT_MUTED).italic(),
        )));
    }
}

fn sidebar_prefixed(text: &str) -> String {
    format!("{SIDEBAR_INDENT}{text}")
}

fn sidebar_label(text: &str) -> String {
    format!("{SIDEBAR_LABEL_INDENT}{text}")
}
