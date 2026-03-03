use std::fs;
use std::io::Cursor;
use std::path::Path;
use std::time::Duration;

use base64::Engine;
use crossterm::event::{
    self, Event, KeyCode, KeyEventKind, KeyModifiers, MouseEvent, MouseEventKind,
};
use ratatui::layout::Rect;

use crate::cli::tui::{self, ChatApp, QuestionKeyResult, TuiEventSender};
use crate::config::Settings;
use crate::core::MessageAttachment;
use crate::session::{SessionEvent, SessionStore};

const INPUT_POLL_TIMEOUT: Duration = Duration::from_millis(16);
const INPUT_BATCH_MAX: usize = 64;

/// Input event from terminal
pub(super) enum InputEvent {
    Key(event::KeyEvent),
    Paste(String),
    ScrollUp { x: u16, y: u16 },
    ScrollDown { x: u16, y: u16 },
    Refresh,
    MouseClick { x: u16, y: u16 },
    MouseDrag { x: u16, y: u16 },
    MouseRelease { x: u16, y: u16 },
}

pub(super) async fn handle_input_batch() -> anyhow::Result<Vec<InputEvent>> {
    if !event::poll(INPUT_POLL_TIMEOUT)? {
        return Ok(Vec::new());
    }

    let mut events = Vec::with_capacity(INPUT_BATCH_MAX.min(8));
    if let Some(input_event) = translate_terminal_event(event::read()?) {
        events.push(input_event);
    }

    while events.len() < INPUT_BATCH_MAX && event::poll(Duration::ZERO)? {
        if let Some(input_event) = translate_terminal_event(event::read()?) {
            events.push(input_event);
        }
    }

    Ok(events)
}

fn translate_terminal_event(event: Event) -> Option<InputEvent> {
    match event {
        Event::Key(key) => Some(InputEvent::Key(key)),
        Event::Paste(text) => Some(InputEvent::Paste(text)),
        Event::Mouse(mouse) => handle_mouse_event(mouse),
        Event::Resize(_, _) | Event::FocusGained => Some(InputEvent::Refresh),
        _ => None,
    }
}

pub(super) fn handle_key_event<F>(
    key_event: event::KeyEvent,
    app: &mut ChatApp,
    settings: &Settings,
    cwd: &Path,
    event_sender: &TuiEventSender,
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
                scroll_up_steps(app, width, height, 1);
            }
            KeyCode::Down => {
                let (width, height) = terminal_size()?;
                scroll_down_once(app, width, height);
            }
            KeyCode::PageUp => {
                let (width, height) = terminal_size()?;
                scroll_up_steps(
                    app,
                    width,
                    height,
                    app.message_viewport_height(height).saturating_sub(1),
                );
            }
            KeyCode::PageDown => {
                let (width, height) = terminal_size()?;
                scroll_page_down(app, width, height);
            }
            _ => {}
        }
        return Ok(());
    }

    if app.is_processing && key_event.code != KeyCode::Esc {
        app.clear_pending_esc_interrupt();
    }

    if app.has_pending_question() {
        let handled = app.handle_question_key(key_event);
        if handled == QuestionKeyResult::Dismissed && app.is_processing {
            if app.should_interrupt_on_esc() {
                app.cancel_agent_task();
                app.set_processing(false);
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
            mutate_input(app, ChatApp::clear_input);
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
                    _ => {}
                }
            } else {
                mutate_input(app, |app| app.insert_char(c));
            }
        }
        KeyCode::Backspace => {
            mutate_input(app, ChatApp::backspace);
        }
        KeyCode::Enter if key_event.modifiers.contains(KeyModifiers::SHIFT) => {
            mutate_input(app, |app| app.insert_char('\n'));
        }
        KeyCode::Enter => {
            handle_enter_key(app, settings, cwd, event_sender);
        }
        KeyCode::Tab => {
            app.cycle_agent();
        }
        KeyCode::Esc => {
            if app.is_processing {
                if app.should_interrupt_on_esc() {
                    app.cancel_agent_task();
                    app.set_processing(false);
                } else {
                    app.arm_esc_interrupt();
                }
            } else {
                mutate_input(app, ChatApp::clear_input);
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
                scroll_up_steps(app, width, height, 1);
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
                scroll_down_once(app, width, height);
            }
        }
        KeyCode::PageUp => {
            let (width, height) = terminal_size()?;
            scroll_up_steps(
                app,
                width,
                height,
                app.message_viewport_height(height).saturating_sub(1),
            );
        }
        KeyCode::PageDown => {
            let (width, height) = terminal_size()?;
            scroll_page_down(app, width, height);
        }
        _ => {}
    }

    Ok(())
}

fn scroll_down_once(app: &mut ChatApp, width: u16, height: u16) {
    scroll_down_steps(app, width, height, 1);
}

pub(super) fn scroll_up_steps(app: &mut ChatApp, width: u16, height: u16, steps: usize) {
    if steps == 0 {
        return;
    }

    let (total_lines, visible_height) = scroll_bounds(app, width, height);
    app.message_scroll
        .scroll_up_steps(total_lines, visible_height, steps);
}

fn scroll_down_steps(app: &mut ChatApp, width: u16, height: u16, steps: usize) {
    if steps == 0 {
        return;
    }

    let (total_lines, visible_height) = scroll_bounds(app, width, height);
    app.message_scroll
        .scroll_down_steps(total_lines, visible_height, steps);
}

fn mutate_input(app: &mut ChatApp, mutator: impl FnOnce(&mut ChatApp)) {
    mutator(app);
    app.update_command_filtering();
}

pub(super) fn apply_paste(app: &mut ChatApp, pasted: String) {
    let mut prepared = prepare_paste(&pasted);
    if prepared.attachments.is_empty()
        && let Some(clipboard_image) = prepare_clipboard_image_paste()
    {
        prepared = clipboard_image;
    }
    apply_prepared_paste(app, prepared);
}

fn apply_prepared_paste(app: &mut ChatApp, prepared: PreparedPaste) {
    mutate_input(app, |app| {
        app.insert_str(&prepared.insert_text);
        for attachment in prepared.attachments {
            app.add_pending_attachment(attachment);
        }
    });
}

pub(super) struct PreparedPaste {
    pub(super) insert_text: String,
    pub(super) attachments: Vec<MessageAttachment>,
}

pub(super) fn prepare_paste(pasted: &str) -> PreparedPaste {
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

fn maybe_handle_paste_shortcut(key_event: event::KeyEvent, app: &mut ChatApp) -> bool {
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

fn selected_command_name(app: &ChatApp) -> Option<String> {
    app.filtered_commands
        .get(app.selected_command_index)
        .map(|command| command.name.clone())
}

fn submit_and_handle(
    app: &mut ChatApp,
    settings: &Settings,
    cwd: &Path,
    event_sender: &TuiEventSender,
) {
    let input = app.submit_input();
    app.update_command_filtering();
    super::handle_submitted_input(input, app, settings, cwd, event_sender);
}

fn handle_enter_key(
    app: &mut ChatApp,
    settings: &Settings,
    cwd: &Path,
    event_sender: &TuiEventSender,
) {
    if let Some(name) = selected_command_name(app)
        && app.input != name
    {
        mutate_input(app, |app| app.set_input(name));
        return;
    }

    submit_and_handle(app, settings, cwd, event_sender);
}

fn scroll_page_down(app: &mut ChatApp, width: u16, height: u16) {
    let (total_lines, visible_height) = scroll_bounds(app, width, height);
    app.message_scroll.scroll_down_steps(
        total_lines,
        visible_height,
        visible_height.saturating_sub(1),
    );
}

fn scroll_bounds(app: &ChatApp, width: u16, height: u16) -> (usize, usize) {
    let visible_height = app.message_viewport_height(height);
    let wrap_width = app.message_wrap_width(width);
    let lines = app.get_lines(wrap_width);
    let total_lines = lines.len();
    drop(lines);
    (total_lines, visible_height)
}

fn copy_selection_to_clipboard(app: &ChatApp, terminal_width: u16) -> bool {
    let wrap_width = app.message_wrap_width(terminal_width);
    let lines = app.get_lines(wrap_width);
    let selected_text = app.get_selected_text(&lines);

    if !selected_text.is_empty()
        && let Ok(mut clipboard) = arboard::Clipboard::new()
        && clipboard.set_text(&selected_text).is_ok()
    {
        return true;
    }

    false
}

pub(super) fn handle_mouse_click(
    app: &mut ChatApp,
    x: u16,
    y: u16,
    terminal: &tui::Tui,
    settings: &Settings,
    cwd: &Path,
) {
    if let Some((line, _column)) = screen_to_message_coords(app, x, y, terminal)
        && let Ok(size) = terminal.size()
    {
        let wrap_width = app.message_wrap_width(size.width);
        if let Some(target) = app.task_session_target_at_visual_line(wrap_width, line)
            && let Ok(messages) = load_session_messages(settings, cwd, &target.session_id)
        {
            let messages = if messages.is_empty() {
                vec![tui::ChatMessage::Assistant(
                    "Subagent is queued or has not emitted messages yet. This view updates automatically once output is available.".to_string(),
                )]
            } else {
                messages
            };
            app.open_subagent_session(target.task_id, target.session_id, target.name, messages);
            return;
        }
    }

    if let Some((line, column)) = screen_to_message_coords(app, x, y, terminal) {
        app.start_selection(line, column);
    }
}

pub(super) fn handle_mouse_drag(app: &mut ChatApp, x: u16, y: u16, terminal: &tui::Tui) {
    if let Some((line, column)) = screen_to_message_coords(app, x, y, terminal) {
        app.update_selection(line, column);
    }
}

pub(super) fn handle_mouse_release(app: &mut ChatApp, x: u16, y: u16, terminal: &tui::Tui) {
    if let Some((line, column)) = screen_to_message_coords(app, x, y, terminal) {
        app.update_selection(line, column);
    }
    if app.text_selection.is_active()
        && let Ok(size) = terminal.size()
    {
        if copy_selection_to_clipboard(app, size.width) {
            app.show_clipboard_notice(x, y);
        }
        app.clear_selection();
    }
    app.end_selection();
}

fn screen_to_message_coords(
    app: &ChatApp,
    x: u16,
    y: u16,
    terminal: &tui::Tui,
) -> Option<(usize, usize)> {
    const MAIN_OUTER_PADDING_X: u16 = 1;
    const MAIN_OUTER_PADDING_Y: u16 = 1;

    let size = terminal.size().ok()?;

    let input_area_height = 6;
    if y < MAIN_OUTER_PADDING_Y || y >= size.height.saturating_sub(input_area_height) {
        return None;
    }

    let relative_y = (y - MAIN_OUTER_PADDING_Y) as usize;
    let relative_x = x.saturating_sub(MAIN_OUTER_PADDING_X) as usize;

    let wrap_width = app.message_wrap_width(size.width);
    let total_lines = app.get_lines(wrap_width).len();
    let visible_height = app.message_viewport_height(size.height);
    let scroll_offset = app
        .message_scroll
        .effective_offset(total_lines, visible_height);

    let line = scroll_offset.saturating_add(relative_y);
    let column = relative_x;

    Some((line, column))
}

pub(super) fn handle_area_scroll(
    app: &mut ChatApp,
    terminal_size: Rect,
    x: u16,
    y: u16,
    up_steps: usize,
    down_steps: usize,
) -> bool {
    let layout_rects = tui::compute_layout_rects(terminal_size, app);

    if let Some(sidebar_content) = layout_rects.sidebar_content
        && point_in_rect(x, y, sidebar_content)
    {
        let total_lines = app.get_sidebar_lines(sidebar_content.width).len();
        let visible_height = sidebar_content.height as usize;

        if total_lines > visible_height {
            if up_steps > 0 {
                app.sidebar_scroll
                    .scroll_up_steps(total_lines, visible_height, up_steps);
            }
            if down_steps > 0 {
                app.sidebar_scroll
                    .scroll_down_steps(total_lines, visible_height, down_steps);
            }
            return true;
        }
        return true;
    }

    if let Some(main_messages) = layout_rects.main_messages
        && point_in_rect(x, y, main_messages)
    {
        let (total_lines, visible_height) =
            scroll_bounds(app, terminal_size.width, terminal_size.height);
        if up_steps > 0 {
            app.message_scroll
                .scroll_up_steps(total_lines, visible_height, up_steps);
        }
        if down_steps > 0 {
            app.message_scroll
                .scroll_down_steps(total_lines, visible_height, down_steps);
        }
        return true;
    }

    false
}

fn point_in_rect(x: u16, y: u16, rect: Rect) -> bool {
    x >= rect.x && x < rect.right() && y >= rect.y && y < rect.bottom()
}

pub(super) fn load_session_messages(
    settings: &Settings,
    cwd: &Path,
    session_id: &str,
) -> anyhow::Result<Vec<tui::ChatMessage>> {
    let store = SessionStore::new(&settings.session.root, cwd, Some(session_id), None)?;
    let events = store.replay_events()?;
    let mut messages = Vec::new();

    for event in events {
        match event {
            SessionEvent::Message { message, .. } => {
                let chat_msg = match message.role {
                    crate::core::Role::User => tui::ChatMessage::User(message.content),
                    crate::core::Role::Assistant => tui::ChatMessage::Assistant(message.content),
                    _ => continue,
                };
                messages.push(chat_msg);
            }
            SessionEvent::ToolCall { call } => {
                messages.push(tui::ChatMessage::ToolCall {
                    name: call.name,
                    args: call.arguments.to_string(),
                    output: None,
                    is_error: None,
                });
            }
            SessionEvent::ToolResult {
                is_error,
                output,
                result,
                ..
            } => {
                for message in messages.iter_mut().rev() {
                    if let tui::ChatMessage::ToolCall {
                        output: existing_output,
                        is_error: existing_status,
                        ..
                    } = message
                        && existing_output.is_none()
                    {
                        *existing_status = Some(is_error);
                        *existing_output =
                            Some(result.clone().map_or(output, |value| value.output));
                        break;
                    }
                }
            }
            SessionEvent::Thinking { content, .. } => {
                messages.push(tui::ChatMessage::Thinking(content));
            }
            SessionEvent::Compact { summary, .. } => {
                messages.push(tui::ChatMessage::Compaction(summary));
            }
            _ => {}
        }
    }

    Ok(messages)
}

pub(super) fn handle_mouse_event(mouse: MouseEvent) -> Option<InputEvent> {
    match mouse.kind {
        MouseEventKind::ScrollUp => Some(InputEvent::ScrollUp {
            x: mouse.column,
            y: mouse.row,
        }),
        MouseEventKind::ScrollDown => Some(InputEvent::ScrollDown {
            x: mouse.column,
            y: mouse.row,
        }),
        MouseEventKind::Down(crossterm::event::MouseButton::Left) => Some(InputEvent::MouseClick {
            x: mouse.column,
            y: mouse.row,
        }),
        MouseEventKind::Drag(crossterm::event::MouseButton::Left) => Some(InputEvent::MouseDrag {
            x: mouse.column,
            y: mouse.row,
        }),
        MouseEventKind::Up(crossterm::event::MouseButton::Left) => Some(InputEvent::MouseRelease {
            x: mouse.column,
            y: mouse.row,
        }),
        _ => None,
    }
}
