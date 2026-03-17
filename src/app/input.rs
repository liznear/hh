use std::fs;
use std::io::Cursor;
use std::path::Path;

use base64::Engine;
use crossterm::event::{self, KeyCode, KeyEventKind, KeyModifiers};
use ratatui::layout::Rect;

use crate::app::chat_state::QuestionKeyResult;
use crate::app::state::AppState;
use crate::core::{Message, MessageAttachment, Role};

#[allow(clippy::too_many_arguments)]
pub(crate) fn handle_key_event<F>(
    key_event: event::KeyEvent,
    app: &mut AppState,
    messages: &mut crate::app::components::messages::MessagesComponent,
    actions: &mut Vec<crate::app::core::AppAction>,
    mut terminal_size: F,
) -> anyhow::Result<()>
where
    F: FnMut() -> anyhow::Result<(u16, u16)>,
{
    if key_event.kind == KeyEventKind::Release {
        return Ok(());
    }

    if app.is_viewing_subagent_session() {
        match key_event.code {
            KeyCode::Esc | KeyCode::Backspace => app.close_subagent_session(),
            KeyCode::Up => {
                let (width, height) = terminal_size()?;
                scroll_up_steps(app, messages, width, height, 1);
            }
            KeyCode::Down => {
                let (width, height) = terminal_size()?;
                scroll_down_once(app, messages, width, height);
            }
            KeyCode::PageUp => {
                let (width, height) = terminal_size()?;
                scroll_up_steps(
                    app,
                    messages,
                    width,
                    height,
                    app.message_viewport_height(height).saturating_sub(1),
                );
            }
            KeyCode::PageDown => {
                let (width, height) = terminal_size()?;
                scroll_page_down(app, messages, width, height);
            }
            _ => {}
        }
        return Ok(());
    }

    if app.context.is_processing && key_event.code != KeyCode::Esc {
        app.clear_pending_esc_interrupt();
    }

    if app.has_pending_question() {
        let handled = app.handle_question_key(key_event);
        if handled == QuestionKeyResult::Dismissed && app.context.is_processing {
            if app.should_interrupt_on_esc() {
                actions.push(crate::app::core::AppAction::CancelExecution);
            } else {
                app.arm_esc_interrupt();
            }
        }
        if handled != QuestionKeyResult::NotHandled {
            return Ok(());
        }
    }

    if key_event.code == KeyCode::Char('c') && key_event.modifiers.contains(KeyModifiers::CONTROL) {
        if app.input.is_empty() {
            app.should_quit = true;
        } else {
            mutate_input(app, AppState::clear_input);
        }
        return Ok(());
    }

    if maybe_handle_paste_shortcut(key_event, app) {
        return Ok(());
    }

    match key_event.code {
        KeyCode::Char(c) => {
            if key_event.modifiers.contains(KeyModifiers::CONTROL) {
                match c {
                    'a' | 'A' => app.move_to_line_start(),
                    'e' | 'E' => app.move_to_line_end(),
                    'j' | 'J' => mutate_input(app, |app| app.insert_char('\n')),
                    _ => {}
                }
            } else {
                mutate_input(app, |app| app.insert_char(c));
            }
        }
        KeyCode::Backspace => {
            mutate_input(app, AppState::backspace);
        }
        KeyCode::Enter if key_event.modifiers.contains(KeyModifiers::SHIFT) => {
            mutate_input(app, |app| app.insert_char('\n'));
        }
        KeyCode::Enter => {
            handle_enter_key(app, actions);
        }
        KeyCode::Tab => {
            app.cycle_agent();
        }
        KeyCode::Esc => {
            if app.context.is_processing {
                if app.should_interrupt_on_esc() {
                    actions.push(crate::app::core::AppAction::CancelExecution);
                } else {
                    app.arm_esc_interrupt();
                }
            } else {
                mutate_input(app, AppState::clear_input);
            }
        }
        KeyCode::Up => {
            if !app.filtered_commands.is_empty() {
                if app.selected_command_index > 0 {
                    app.selected_command_index -= 1;
                } else {
                    app.selected_command_index = app.filtered_commands.len().saturating_sub(1);
                }
            } else if !app.input.is_empty() {
                app.move_cursor_up();
            } else {
                let (width, height) = terminal_size()?;
                scroll_up_steps(app, messages, width, height, 1);
            }
        }
        KeyCode::Left => {
            app.move_cursor_left();
        }
        KeyCode::Right => {
            app.move_cursor_right();
        }
        KeyCode::Down => {
            if !app.filtered_commands.is_empty() {
                if app.selected_command_index < app.filtered_commands.len().saturating_sub(1) {
                    app.selected_command_index += 1;
                } else {
                    app.selected_command_index = 0;
                }
            } else if !app.input.is_empty() {
                app.move_cursor_down();
            } else {
                let (width, height) = terminal_size()?;
                scroll_down_once(app, messages, width, height);
            }
        }
        KeyCode::PageUp => {
            let (width, height) = terminal_size()?;
            scroll_up_steps(
                app,
                messages,
                width,
                height,
                app.message_viewport_height(height).saturating_sub(1),
            );
        }
        KeyCode::PageDown => {
            let (width, height) = terminal_size()?;
            scroll_page_down(app, messages, width, height);
        }
        _ => {}
    }

    Ok(())
}

fn scroll_down_once(
    app: &mut AppState,
    messages: &mut crate::app::components::messages::MessagesComponent,
    width: u16,
    height: u16,
) {
    scroll_down_steps(app, messages, width, height, 1);
}

pub(crate) fn scroll_up_steps(
    app: &mut AppState,
    messages: &mut crate::app::components::messages::MessagesComponent,
    width: u16,
    height: u16,
    steps: usize,
) {
    if steps == 0 {
        return;
    }

    let (total_lines, visible_height) = scroll_bounds(app, messages, width, height);
    app.message_scroll
        .scroll_up_steps(total_lines, visible_height, steps);
}

fn scroll_down_steps(
    app: &mut AppState,
    messages: &mut crate::app::components::messages::MessagesComponent,
    width: u16,
    height: u16,
    steps: usize,
) {
    if steps == 0 {
        return;
    }

    let (total_lines, visible_height) = scroll_bounds(app, messages, width, height);
    app.message_scroll
        .scroll_down_steps(total_lines, visible_height, steps);
}

fn mutate_input(app: &mut AppState, mutator: impl FnOnce(&mut AppState)) {
    mutator(app);
    app.update_command_filtering();
}

pub(crate) fn apply_paste(app: &mut AppState, pasted: String) {
    let mut prepared = prepare_paste(&pasted);
    if prepared.attachments.is_empty()
        && let Some(clipboard_image) = prepare_clipboard_image_paste()
    {
        prepared = clipboard_image;
    }
    apply_prepared_paste(app, prepared);
}

fn apply_prepared_paste(app: &mut AppState, prepared: PreparedPaste) {
    mutate_input(app, |app| {
        app.insert_str(&prepared.insert_text);
        for attachment in prepared.attachments {
            app.add_pending_attachment(attachment);
        }
    });
}

pub(crate) struct PreparedPaste {
    pub(crate) insert_text: String,
    pub(crate) attachments: Vec<MessageAttachment>,
}

pub(crate) fn prepare_paste(pasted: &str) -> PreparedPaste {
    if let Some(image_paste) = prepare_image_file_paste(pasted) {
        return image_paste;
    }

    PreparedPaste {
        insert_text: pasted.to_string(),
        attachments: Vec::new(),
    }
}

fn prepare_image_file_paste(pasted: &str) -> Option<PreparedPaste> {
    let non_empty_lines: Vec<&str> = pasted
        .lines()
        .filter(|line| !line.trim().is_empty())
        .collect();
    if non_empty_lines.is_empty() {
        return None;
    }

    let mut image_paths = Vec::with_capacity(non_empty_lines.len());
    let mut attachments = Vec::with_capacity(non_empty_lines.len());
    for line in &non_empty_lines {
        let path = extract_image_path(line)?;
        let attachment = read_image_file_attachment(&path)?;
        image_paths.push(path);
        attachments.push(attachment);
    }

    let insert_text = image_paths
        .iter()
        .enumerate()
        .map(|(idx, path)| {
            let name = Path::new(path)
                .file_name()
                .and_then(|value| value.to_str())
                .unwrap_or("image");
            if image_paths.len() == 1 {
                format!("[pasted image: {name}]")
            } else {
                format!("[pasted image {}: {name}]", idx + 1)
            }
        })
        .collect::<Vec<_>>()
        .join("\n");

    Some(PreparedPaste {
        insert_text,
        attachments,
    })
}

fn maybe_handle_paste_shortcut(key_event: event::KeyEvent, app: &mut AppState) -> bool {
    if !is_paste_shortcut(key_event) {
        return false;
    }

    if let Some(prepared) = prepare_clipboard_image_paste() {
        apply_prepared_paste(app, prepared);
        return true;
    }

    if let Some(text) = read_clipboard_text() {
        apply_paste(app, text);
    }

    true
}

fn is_paste_shortcut(key_event: event::KeyEvent) -> bool {
    (key_event.code == KeyCode::Char('v')
        && (key_event.modifiers.contains(KeyModifiers::CONTROL)
            || key_event.modifiers.contains(KeyModifiers::SUPER)))
        || (key_event.code == KeyCode::Insert && key_event.modifiers.contains(KeyModifiers::SHIFT))
}

fn prepare_clipboard_image_paste() -> Option<PreparedPaste> {
    let mut clipboard = arboard::Clipboard::new().ok()?;
    let image = clipboard.get_image().ok()?;
    let png_data = encode_rgba_to_png(image.width, image.height, image.bytes.as_ref())?;
    let data_base64 = base64::engine::general_purpose::STANDARD.encode(png_data);

    Some(PreparedPaste {
        insert_text: "[pasted image from clipboard]".to_string(),
        attachments: vec![MessageAttachment::Image {
            media_type: "image/png".to_string(),
            data_base64,
        }],
    })
}

fn read_clipboard_text() -> Option<String> {
    let mut clipboard = arboard::Clipboard::new().ok()?;
    let text = clipboard.get_text().ok()?;
    if text.is_empty() { None } else { Some(text) }
}

fn encode_rgba_to_png(width: usize, height: usize, rgba_bytes: &[u8]) -> Option<Vec<u8>> {
    let mut output = Vec::new();
    {
        let mut cursor = Cursor::new(&mut output);
        let mut encoder = png::Encoder::new(&mut cursor, width as u32, height as u32);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        let mut writer = encoder.write_header().ok()?;
        writer.write_image_data(rgba_bytes).ok()?;
    }
    Some(output)
}

fn extract_image_path(raw: &str) -> Option<String> {
    let trimmed = strip_surrounding_quotes(raw.trim());
    if trimmed.is_empty() {
        return None;
    }

    let normalized = if let Some(rest) = trimmed.strip_prefix("file://") {
        let path = if rest.starts_with('/') {
            rest
        } else {
            return None;
        };
        match urlencoding::decode(path) {
            Ok(decoded) => decoded.into_owned(),
            Err(_) => return None,
        }
    } else {
        trimmed.to_string()
    };

    resolve_image_path(&normalized)
}

fn resolve_image_path(path: &str) -> Option<String> {
    let unescaped = unescape_shell_escaped_path(path);
    let mut candidates = vec![path.to_string()];
    if unescaped != path {
        candidates.push(unescaped);
    }

    for candidate in &candidates {
        if is_image_path(candidate) && Path::new(candidate).exists() {
            return Some(candidate.clone());
        }
    }

    candidates
        .into_iter()
        .find(|candidate| is_image_path(candidate))
}

fn unescape_shell_escaped_path(path: &str) -> String {
    let mut out = String::with_capacity(path.len());
    let mut chars = path.chars();
    while let Some(ch) = chars.next() {
        if ch == '\\' {
            if let Some(next) = chars.next() {
                out.push(next);
            } else {
                out.push('\\');
            }
        } else {
            out.push(ch);
        }
    }
    out
}

fn read_image_file_attachment(path: &str) -> Option<MessageAttachment> {
    let media_type = image_media_type(path)?;
    let bytes = fs::read(path).ok()?;
    let data_base64 = base64::engine::general_purpose::STANDARD.encode(bytes);
    Some(MessageAttachment::Image {
        media_type: media_type.to_string(),
        data_base64,
    })
}

fn image_media_type(path: &str) -> Option<&'static str> {
    let lower = path.to_ascii_lowercase();
    if lower.ends_with(".png") {
        Some("image/png")
    } else if lower.ends_with(".jpg") || lower.ends_with(".jpeg") {
        Some("image/jpeg")
    } else if lower.ends_with(".gif") {
        Some("image/gif")
    } else if lower.ends_with(".webp") {
        Some("image/webp")
    } else if lower.ends_with(".bmp") {
        Some("image/bmp")
    } else if lower.ends_with(".tiff") || lower.ends_with(".tif") {
        Some("image/tiff")
    } else if lower.ends_with(".heic") {
        Some("image/heic")
    } else if lower.ends_with(".heif") {
        Some("image/heif")
    } else if lower.ends_with(".avif") {
        Some("image/avif")
    } else {
        None
    }
}

fn strip_surrounding_quotes(value: &str) -> &str {
    if value.len() < 2 {
        return value;
    }
    let bytes = value.as_bytes();
    let first = bytes[0];
    let last = bytes[value.len() - 1];
    if (first == b'\'' && last == b'\'') || (first == b'"' && last == b'"') {
        &value[1..value.len() - 1]
    } else {
        value
    }
}

fn is_image_path(path: &str) -> bool {
    let lower = path.to_ascii_lowercase();
    [
        ".png", ".jpg", ".jpeg", ".gif", ".webp", ".bmp", ".tiff", ".tif", ".heic", ".heif",
        ".avif",
    ]
    .iter()
    .any(|ext| lower.ends_with(ext))
}

fn selected_command_name(app: &AppState) -> Option<String> {
    app.filtered_commands
        .get(app.selected_command_index)
        .map(|command| command.name.clone())
}

fn submit_and_handle(app: &mut AppState, actions: &mut Vec<crate::app::core::AppAction>) {
    let input = app.submit_input();
    app.update_command_filtering();

    if input.queued {
        let Some(message_index) = input.message_index else {
            return;
        };
        actions.push(crate::app::core::AppAction::QueueUserMessage {
            message: Message {
                role: Role::User,
                content: input.text,
                attachments: input.attachments,
                tool_call_id: None,
                tool_calls: Vec::new(),
            },
            message_index,
        });
        return;
    }

    actions.push(crate::app::core::AppAction::SubmitInput(
        input.text,
        input.attachments,
    ));
}

fn handle_enter_key(app: &mut AppState, actions: &mut Vec<crate::app::core::AppAction>) {
    if let Some(name) = selected_command_name(app)
        && app.input != name
    {
        mutate_input(app, |app| app.set_input(name));
        return;
    }

    submit_and_handle(app, actions);
}

fn scroll_page_down(
    app: &mut AppState,
    messages: &mut crate::app::components::messages::MessagesComponent,
    width: u16,
    height: u16,
) {
    let (total_lines, visible_height) = scroll_bounds(app, messages, width, height);
    app.message_scroll.scroll_down_steps(
        total_lines,
        visible_height,
        visible_height.saturating_sub(1),
    );
}

fn scroll_bounds(
    app: &AppState,
    messages: &mut crate::app::components::messages::MessagesComponent,
    width: u16,
    height: u16,
) -> (usize, usize) {
    let visible_height = app.message_viewport_height(height);
    let wrap_width = app.message_wrap_width(width);
    let lines = messages.viewport.get_lines(app, wrap_width);
    let total_lines = lines.len();
    (total_lines, visible_height)
}

fn copy_selection_to_clipboard(
    app: &AppState,
    messages: &mut crate::app::components::messages::MessagesComponent,
    terminal_width: u16,
) -> bool {
    let wrap_width = app.message_wrap_width(terminal_width);
    let lines = messages.viewport.get_lines(app, wrap_width);
    let selected_text = app.get_selected_text(lines);

    if !selected_text.is_empty()
        && let Ok(mut clipboard) = arboard::Clipboard::new()
        && clipboard.set_text(&selected_text).is_ok()
    {
        return true;
    }

    false
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn handle_mouse_click(
    app: &mut AppState,
    messages: &mut crate::app::components::messages::MessagesComponent,
    sidebar: &crate::app::components::sidebar::SidebarComponent,
    actions: &mut Vec<crate::app::core::AppAction>,
    x: u16,
    y: u16,
    terminal: &impl crate::app::runtime::TerminalBackend,
) {
    if let Some(section_id) = screen_to_sidebar_header(app, sidebar, x, y, terminal) {
        actions.push(crate::app::core::AppAction::ToggleSidebarSection(
            section_id.to_string(),
        ));
        return;
    }

    if let Some((line, _column)) = screen_to_message_coords(app, messages, x, y, terminal)
        && let Ok(size) = terminal.size()
    {
        let wrap_width = app.message_wrap_width(size.0);

        if let Some(target) = app.task_session_target_at_visual_line(wrap_width, line) {
            actions.push(crate::app::core::AppAction::OpenSubagentSession {
                task_id: target.task_id,
                session_id: target.session_id,
                name: target.name,
            });
            return;
        }
    }

    if let Some((line, column)) = screen_to_message_coords(app, messages, x, y, terminal) {
        app.start_selection(line, column);
    }
}

pub(crate) fn handle_mouse_drag(
    app: &mut AppState,
    messages: &mut crate::app::components::messages::MessagesComponent,
    x: u16,
    y: u16,
    terminal: &impl crate::app::runtime::TerminalBackend,
) {
    if let Some((line, column)) = screen_to_message_coords(app, messages, x, y, terminal) {
        app.update_selection(line, column);
    }
}

pub(crate) fn handle_mouse_release(
    app: &mut AppState,
    messages: &mut crate::app::components::messages::MessagesComponent,
    x: u16,
    y: u16,
    terminal: &impl crate::app::runtime::TerminalBackend,
) -> Option<crate::app::core::AppAction> {
    let mut action = None;
    if let Some((line, column)) = screen_to_message_coords(app, messages, x, y, terminal) {
        app.update_selection(line, column);
    }
    if app.text_selection.is_active()
        && let Ok(size) = terminal.size()
    {
        if copy_selection_to_clipboard(app, messages, size.0) {
            action = Some(crate::app::core::AppAction::ShowClipboardNotice { x, y });
        }
        app.clear_selection();
    }
    app.end_selection();
    action
}

fn screen_to_message_coords(
    app: &AppState,
    messages: &mut crate::app::components::messages::MessagesComponent,
    x: u16,
    y: u16,
    terminal: &impl crate::app::runtime::TerminalBackend,
) -> Option<(usize, usize)> {
    let size = terminal.size().ok()?;
    let terminal_rect = ratatui::layout::Rect::new(0, 0, size.0, size.1);
    let layout_rects =
        crate::app::components::layout::compute_layout_rects(terminal_rect, app, &app.input);

    let main_messages = layout_rects.main_messages?;
    if !point_in_rect(x, y, main_messages) {
        return None;
    }

    let relative_y = (y - main_messages.y) as usize;
    let relative_x = (x - main_messages.x) as usize;

    let wrap_width = app.message_wrap_width(size.0);
    let total_lines = messages.viewport.get_lines(app, wrap_width).len();
    let visible_height = main_messages.height as usize;
    let scroll_offset = app
        .message_scroll
        .effective_offset(total_lines, visible_height);

    let line = scroll_offset.saturating_add(relative_y);
    let column = relative_x;

    Some((line, column))
}

fn screen_to_sidebar_header(
    app: &AppState,
    sidebar: &crate::app::components::sidebar::SidebarComponent,
    x: u16,
    y: u16,
    terminal: &impl crate::app::runtime::TerminalBackend,
) -> Option<&'static str> {
    let size = terminal.size().ok()?;
    let terminal_rect = ratatui::layout::Rect::new(0, 0, size.0, size.1);
    let layout_rects =
        crate::app::components::layout::compute_layout_rects(terminal_rect, app, &app.input);
    let sidebar_content = layout_rects.sidebar_content?;

    if !point_in_rect(x, y, sidebar_content) {
        return None;
    }

    let relative_y = (y - sidebar_content.y) as usize;
    let relative_x = x - sidebar_content.x;

    let scroll_offset = {
        let lines = crate::app::components::sidebar::build_sidebar_lines(
            app,
            sidebar,
            sidebar_content.width,
        );
        sidebar
            .scroll
            .effective_offset(lines.len(), sidebar_content.height as usize)
    };
    let line_index = scroll_offset.saturating_add(relative_y);

    crate::app::components::sidebar::sidebar_section_header_hitboxes(
        app,
        sidebar,
        sidebar_content.width,
    )
    .into_iter()
    .find(|hitbox| {
        hitbox.line_index == line_index && relative_x < hitbox.title_width && hitbox.title_width > 0
    })
    .map(|hitbox| hitbox.section_id)
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn handle_area_scroll(
    app: &mut AppState,
    messages: &mut crate::app::components::messages::MessagesComponent,
    sidebar: &crate::app::components::sidebar::SidebarComponent,
    actions: &mut Vec<crate::app::core::AppAction>,
    terminal_size: Rect,
    x: u16,
    y: u16,
    up_steps: usize,
    down_steps: usize,
) -> bool {
    let layout_rects =
        crate::app::components::layout::compute_layout_rects(terminal_size, app, &app.input);

    if let Some(sidebar_content) = layout_rects.sidebar_content
        && point_in_rect(x, y, sidebar_content)
    {
        let total_lines = crate::app::components::sidebar::build_sidebar_lines(
            app,
            sidebar,
            sidebar_content.width,
        )
        .len();
        let visible_height = sidebar_content.height as usize;

        if total_lines > visible_height {
            if up_steps > 0 {
                actions.push(crate::app::core::AppAction::ScrollSidebar(
                    -(up_steps as i32),
                ));
            }
            if down_steps > 0 {
                actions.push(crate::app::core::AppAction::ScrollSidebar(
                    down_steps as i32,
                ));
            }
            return true;
        }
        return true;
    }

    if let Some(main_messages) = layout_rects.main_messages
        && point_in_rect(x, y, main_messages)
    {
        let (_total_lines, _visible_height) =
            scroll_bounds(app, messages, terminal_size.width, terminal_size.height);
        if up_steps > 0 {
            actions.push(crate::app::core::AppAction::ScrollMessages(
                -(up_steps as i32),
            ));
        }
        if down_steps > 0 {
            actions.push(crate::app::core::AppAction::ScrollMessages(
                down_steps as i32,
            ));
        }
        return true;
    }

    false
}

fn point_in_rect(x: u16, y: u16, rect: Rect) -> bool {
    x >= rect.x && x < rect.right() && y >= rect.y && y < rect.bottom()
}
