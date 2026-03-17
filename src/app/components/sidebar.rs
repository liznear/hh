use crate::ui_compat::{
    Frame,
        style::Style,
    text::{Line, Span, Text},
    widgets::{Block, Paragraph, Wrap},
};
use serde_json::Value;

use crate::app::chat_state::ScrollState;
use crate::app::chat_state::{ChatMessage, TodoItemView, TodoStatus};
use crate::app::core::{AppAction, Component};
use crate::app::state::AppState;
use crate::theme::colors::*;
use std::collections::HashSet;

pub struct SidebarComponent {
    pub scroll: ScrollState,
    pub folded_sections: HashSet<String>,
    // Cache key inputs: `cached_width` + `cached_app_generation` + `needs_rebuild`.
    // Invalidated by: fold/unfold actions (mark_dirty), message generation changes,
    // and width changes observed at render time.
    // Fallback behavior: rebuild full sidebar lines from current AppState.
    pub cached_lines: Vec<Line<'static>>,
    pub cached_width: u16,
    pub cached_app_generation: u64,
    pub needs_rebuild: bool,
}

impl Default for SidebarComponent {
    fn default() -> Self {
        Self {
            scroll: ScrollState::new(false),
            folded_sections: HashSet::new(),
            cached_lines: Vec::new(),
            cached_width: 0,
            cached_app_generation: 0,
            needs_rebuild: true,
        }
    }
}

impl SidebarComponent {
    pub fn is_folded(&self, section_id: &str) -> bool {
        self.folded_sections.contains(section_id)
    }

    pub fn mark_dirty(&mut self) {
        self.needs_rebuild = true;
    }

    pub(crate) fn render_sidebar(
        &mut self,
        f: &mut Frame,
        app: &AppState,
        area: crate::ui_compat::layout::Rect,
    ) {
        render_sidebar_local(f, app, self, area);
    }

    pub(crate) fn render_sidebar_clipped_to_bottom(
        &mut self,
        f: &mut Frame,
        app: &AppState,
        sidebar_area: crate::ui_compat::layout::Rect,
        bottom_y: u16,
    ) {
        let clipped_sidebar_area = crate::ui_compat::layout::Rect {
            x: sidebar_area.x,
            y: sidebar_area.y,
            width: sidebar_area.width,
            height: bottom_y.saturating_sub(sidebar_area.y),
        };

        if clipped_sidebar_area.width == 0 || clipped_sidebar_area.height == 0 {
            return;
        }

        self.render_sidebar(f, app, clipped_sidebar_area);
    }
}

impl Component for SidebarComponent {
    fn update(&mut self, action: &AppAction) -> Option<AppAction> {
        match action {
            AppAction::ScrollSidebar(amount) => {
                let amount = *amount;
                if amount < 0 {
                    self.scroll.offset = self
                        .scroll
                        .offset
                        .saturating_add(amount.unsigned_abs() as usize);
                } else {
                    self.scroll.offset = self.scroll.offset.saturating_sub(amount as usize);
                }
                Some(AppAction::Redraw)
            }
            AppAction::ToggleSidebarSection(section_id) => {
                if !self.folded_sections.insert(section_id.clone()) {
                    self.folded_sections.remove(section_id);
                }
                self.mark_dirty();
                Some(AppAction::Redraw)
            }
            AppAction::Redraw | AppAction::PeriodicTick => {
                // Since this runs during update, we can't rebuild cache yet until render provides width.
                // It just tells us state changed somewhere.
                None
            }
            _ => None,
        }
    }
}

const FOLDABLE_SECTION_MIN_LINES: usize = 7;

#[derive(Clone, Copy)]
pub(crate) struct SidebarSectionHeaderHitbox {
    pub(crate) section_id: &'static str,
    pub(crate) line_index: usize,
    pub(crate) title_width: u16,
}

fn render_sidebar_local(
    f: &mut Frame,
    app: &AppState,
    sidebar_comp: &mut SidebarComponent,
    area: crate::ui_compat::layout::Rect,
) {
    let block = Block::default().style(Style::default().bg(SIDEBAR_BG));
    let inner = block.inner(area);
    let content = crate::ui_compat::layout::Rect {
        x: inner.x.saturating_add(2).min(inner.right()),
        y: inner.y,
        width: inner.width.saturating_sub(2),
        height: inner.height,
    };
    f.render_widget(block, area);

    let app_generation = app.message_cache_generation();
    let needs_rebuild = sidebar_comp.needs_rebuild
        || sidebar_comp.cached_width != content.width
        || sidebar_comp.cached_app_generation != app_generation;
    if needs_rebuild {
        let lines = build_sidebar_lines(app, sidebar_comp, content.width);
        sidebar_comp.cached_lines = lines;
        sidebar_comp.cached_width = content.width;
        sidebar_comp.cached_app_generation = app_generation;
        sidebar_comp.needs_rebuild = false;
    }

    let lines_ref = &sidebar_comp.cached_lines;
    let scroll_offset = sidebar_comp
        .scroll
        .effective_offset(lines_ref.len(), content.height as usize);

    let sidebar = Paragraph::new(Text::from(lines_ref.to_vec()))
        .style(Style::default().bg(SIDEBAR_BG))
        .wrap(Wrap { trim: true })
        .scroll((scroll_offset as u16, 0));
    f.render_widget(sidebar, content);
}

pub(crate) fn build_sidebar_lines(
    app: &AppState,
    sidebar: &SidebarComponent,
    content_width: u16,
) -> Vec<Line<'static>> {
    build_sidebar_model(app, sidebar, content_width).lines
}

pub(crate) fn sidebar_section_header_hitboxes(
    app: &AppState,
    sidebar: &SidebarComponent,
    content_width: u16,
) -> Vec<SidebarSectionHeaderHitbox> {
    build_sidebar_model(app, sidebar, content_width).hitboxes
}

struct SidebarSection {
    id: &'static str,
    title: String,
    title_style: Style,
    body_lines: Vec<Line<'static>>,
}

impl SidebarSection {
    fn total_lines(&self) -> usize {
        1 + self.body_lines.len()
    }
}

struct SidebarRenderModel {
    lines: Vec<Line<'static>>,
    hitboxes: Vec<SidebarSectionHeaderHitbox>,
}

fn build_sidebar_model(
    app: &AppState,
    sidebar: &SidebarComponent,
    content_width: u16,
) -> SidebarRenderModel {
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
        format_sidebar_directory(&app.cwd.display().to_string(), app.git_branch.as_deref());
    let mut lines: Vec<Line<'static>> = vec![Line::from("")];
    let mut hitboxes = Vec::new();

    lines.push(Line::from(Span::styled(
        sidebar_prefixed(&app.session_name),
        Style::default().fg(TEXT_PRIMARY).bold(),
    )));
    lines.push(Line::from(""));
    for title in app.subagent_session_titles() {
        lines.push(Line::from(Span::styled(
            sidebar_prefixed(&format!("→ {title}")),
            Style::default().fg(TEXT_PRIMARY),
        )));
    }
    if app.subagent_session_depth() > 0 {
        lines.push(Line::from(""));
    }

    lines.push(Line::from(Span::styled(
        sidebar_prefixed(&abbreviate_path(
            &directory_text,
            content_width.saturating_sub(2) as usize,
        )),
        Style::default().fg(TEXT_PRIMARY),
    )));
    lines.push(Line::from(""));

    let mut sections = vec![SidebarSection {
        id: "context",
        title: "Context".to_string(),
        title_style: Style::default().fg(TEXT_SECONDARY).bold(),
        body_lines: vec![Line::from(Span::styled(
            sidebar_prefixed(&format!("{} / {} ({}%)", used, budget, context_percent)),
            Style::default().fg(context_usage_color),
        ))],
    }];

    let modified_files = collect_modified_files(&app.messages);
    if !modified_files.is_empty() {
        let mut modified_lines = Vec::new();
        append_modified_file_list(&mut modified_lines, &modified_files, content_width as usize);
        sections.push(SidebarSection {
            id: "modified_files",
            title: "Modified Files".to_string(),
            title_style: Style::default().fg(TEXT_SECONDARY).bold(),
            body_lines: modified_lines,
        });
    }

    if !app.todo_items.is_empty() {
        let done = app
            .todo_items
            .iter()
            .filter(|item| item.status == TodoStatus::Completed)
            .count();

        let mut todo_lines = vec![Line::from(Span::styled(
            sidebar_label(&format!("{} / {} done", done, app.todo_items.len())),
            Style::default().fg(TEXT_MUTED),
        ))];

        append_sidebar_list(&mut todo_lines, &app.todo_items, app.todo_items.len());
        sections.push(SidebarSection {
            id: "todo",
            title: format!("TODO ({} / {})", done, app.todo_items.len()),
            title_style: Style::default().fg(TEXT_SECONDARY).bold(),
            body_lines: todo_lines,
        });
    }

    let section_count = sections.len();
    for (index, section) in sections.into_iter().enumerate() {
        let is_foldable = section.total_lines() >= FOLDABLE_SECTION_MIN_LINES;
        let is_folded = is_foldable && sidebar.is_folded(section.id);
        let title_text = if is_foldable {
            if is_folded {
                format!("▶ {}", section.title)
            } else {
                format!("▼ {}", section.title)
            }
        } else {
            section.title.to_string()
        };
        let rendered_title = sidebar_label(&title_text);
        let title_width = rendered_title.chars().count() as u16;
        let line_index = lines.len();
        lines.push(Line::from(Span::styled(
            rendered_title,
            section.title_style,
        )));

        if is_foldable {
            hitboxes.push(SidebarSectionHeaderHitbox {
                section_id: section.id,
                line_index,
                title_width,
            });
        }

        if !is_folded {
            lines.extend(section.body_lines);
        }

        if index + 1 < section_count {
            lines.push(Line::from(""));
        }
    }

    SidebarRenderModel { lines, hitboxes }
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
        let path_text = truncate_chars(&file.path, path_max.max(1));
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
        let (marker, marker_style, item_style) = match item.status {
            TodoStatus::Pending => (
                "[ ] ",
                Style::default().fg(INPUT_ACCENT),
                Style::default().fg(TEXT_PRIMARY),
            ),
            TodoStatus::InProgress => (
                "[ ] ",
                Style::default().fg(INPUT_ACCENT),
                Style::default().fg(TODO_ACTIVE_FG),
            ),
            TodoStatus::Completed => (
                "[✓] ",
                Style::default().fg(INPUT_ACCENT),
                Style::default().fg(TEXT_MUTED),
            ),
            TodoStatus::Cancelled => (
                "[-] ",
                Style::default().fg(INPUT_ACCENT),
                Style::default().fg(TEXT_MUTED),
            ),
        };

        lines.push(Line::from(vec![
            Span::styled(sidebar_prefixed(marker), marker_style),
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

fn truncate_chars(input: &str, max_chars: usize) -> String {
    input.chars().take(max_chars).collect()
}
