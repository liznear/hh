use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::OnceLock;
use std::time::Duration;
use std::{fs, io::Cursor};

use base64::Engine;
use crossterm::event::{
    self, Event, KeyCode, KeyEventKind, KeyModifiers, MouseEvent, MouseEventKind,
};
use ratatui::layout::Rect;
use tokio::sync::mpsc;
use tokio::sync::oneshot;

use crate::agent::{AgentLoader, AgentMode, AgentRegistry};
use crate::cli::agent_init;
use crate::cli::render;
use crate::cli::tui::{
    self, ChatApp, ModelOptionView, QuestionKeyResult, ScopedTuiEvent, SubmittedInput, TuiEvent,
    TuiEventSender,
};
use crate::config::Settings;
use crate::core::agent::subagent_manager::{
    SubagentExecutionRequest, SubagentExecutionResult, SubagentExecutor, SubagentManager,
    SubagentStatus,
};
use crate::core::agent::{AgentEvents, AgentLoop, NoopEvents};
use crate::core::{Message, MessageAttachment, Role};
use crate::permission::PermissionMatcher;
use crate::provider::openai_compatible::OpenAiCompatibleProvider;
use crate::session::types::SubAgentFailureReason;
use crate::session::{SessionEvent, SessionStore, event_id};
use crate::tool::registry::{ToolRegistry, ToolRegistryContext};
use crate::tool::task::TaskToolRuntimeContext;
use uuid::Uuid;

static GLOBAL_SUBAGENT_MANAGER: OnceLock<Arc<SubagentManager>> = OnceLock::new();

pub async fn run_chat(settings: Settings, cwd: &std::path::Path) -> anyhow::Result<()> {
    // Setup terminal
    let terminal = tui::setup_terminal()?;
    let mut tui_guard = tui::TuiGuard::new(terminal);

    // Create app state and event channel
    let mut app = ChatApp::new(build_session_name(cwd), cwd);
    app.configure_models(
        settings.selected_model_ref().to_string(),
        build_model_options(&settings),
    );

    // Initialize agents
    let (agent_views, selected_agent) = agent_init::initialize_agents(&settings)?;
    app.set_agents(agent_views, selected_agent);

    let (event_tx, mut event_rx) = mpsc::unbounded_channel::<ScopedTuiEvent>();
    let event_sender = TuiEventSender::new(event_tx);
    initialize_subagent_manager(settings.clone(), cwd.to_path_buf());

    run_interactive_chat_loop(
        &mut tui_guard,
        &mut app,
        InteractiveChatRunner {
            settings: &settings,
            cwd,
            event_sender: &event_sender,
            event_rx: &mut event_rx,
            scroll_down_lines: 3,
        },
    )
    .await?;

    Ok(())
}

/// Input event from terminal
enum InputEvent {
    Key(event::KeyEvent),
    Paste(String),
    ScrollUp { x: u16, y: u16 },
    ScrollDown { x: u16, y: u16 },
    Refresh,
    MouseClick { x: u16, y: u16 },
    MouseDrag { x: u16, y: u16 },
    MouseRelease { x: u16, y: u16 },
}

const INPUT_POLL_TIMEOUT: Duration = Duration::from_millis(16);
const INPUT_BATCH_MAX: usize = 64;
const EVENT_DRAIN_MAX: usize = 128;
const STREAM_CHUNK_FLUSH_INTERVAL: Duration = Duration::from_millis(75);
const STREAM_CHUNK_FLUSH_BYTES: usize = 8192;

async fn handle_input_batch() -> anyhow::Result<Vec<InputEvent>> {
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

fn handle_key_event<F>(
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
                // Clear input when not processing
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

fn scroll_up_steps(app: &mut ChatApp, width: u16, height: u16, steps: usize) {
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

fn apply_paste(app: &mut ChatApp, pasted: String) {
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

struct PreparedPaste {
    insert_text: String,
    attachments: Vec<MessageAttachment>,
}

fn prepare_paste(pasted: &str) -> PreparedPaste {
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
    handle_submitted_input(input, app, settings, cwd, event_sender);
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

/// Copy selected text to clipboard
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

/// Handle mouse click - start text selection
fn handle_mouse_click(app: &mut ChatApp, x: u16, y: u16, terminal: &tui::Tui) {
    if let Some((line, column)) = screen_to_message_coords(app, x, y, terminal) {
        app.start_selection(line, column);
    }
}

/// Handle mouse drag - update text selection
fn handle_mouse_drag(app: &mut ChatApp, x: u16, y: u16, terminal: &tui::Tui) {
    if let Some((line, column)) = screen_to_message_coords(app, x, y, terminal) {
        app.update_selection(line, column);
    }
}

/// Handle mouse release - end text selection
fn handle_mouse_release(app: &mut ChatApp, _x: u16, _y: u16, _terminal: &tui::Tui) {
    if let Some((line, column)) = screen_to_message_coords(app, _x, _y, _terminal) {
        app.update_selection(line, column);
    }
    if app.text_selection.is_active()
        && let Ok(size) = _terminal.size()
    {
        if copy_selection_to_clipboard(app, size.width) {
            app.show_clipboard_notice(_x, _y);
        }
        app.clear_selection();
    }
    app.end_selection();
}

/// Convert screen coordinates to message line and column
fn screen_to_message_coords(
    app: &ChatApp,
    x: u16,
    y: u16,
    terminal: &tui::Tui,
) -> Option<(usize, usize)> {
    const MAIN_OUTER_PADDING_X: u16 = 1;
    const MAIN_OUTER_PADDING_Y: u16 = 1;

    let size = terminal.size().ok()?;

    // Simplified calculation - just check if it's roughly in the message area
    // The message area is at the top, below it are processing indicator and input
    let input_area_height = 6; // Approximate input area height
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

fn handle_area_scroll(
    app: &mut ChatApp,
    terminal_size: Rect,
    x: u16,
    y: u16,
    up_steps: usize,
    down_steps: usize,
) -> bool {
    let layout_rects = tui::compute_layout_rects(terminal_size, app);

    // Check if mouse is in sidebar
    if let Some(sidebar_content) = layout_rects.sidebar_content
        && point_in_rect(x, y, sidebar_content)
    {
        let total_lines = app.get_sidebar_lines(sidebar_content.width).len();
        let visible_height = sidebar_content.height as usize;

        // Only scroll if sidebar has scrollable content
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
        // Sidebar not scrollable, don't scroll anything
        return true;
    }

    // Check if mouse is in main messages area
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

    // Mouse not in a scrollable area
    false
}

fn point_in_rect(x: u16, y: u16, rect: Rect) -> bool {
    x >= rect.x && x < rect.right() && y >= rect.y && y < rect.bottom()
}

fn spawn_agent_task(
    settings: &Settings,
    cwd: &Path,
    input: Message,
    model_ref: String,
    event_sender: &TuiEventSender,
    subagent_manager: Arc<SubagentManager>,
    run_options: AgentRunOptions,
) -> tokio::task::JoinHandle<()> {
    let settings = settings.clone();
    let cwd = cwd.to_path_buf();
    let sender = event_sender.clone();
    tokio::spawn(async move {
        if let Err(e) = run_agent(
            settings,
            &cwd,
            input,
            model_ref,
            sender.clone(),
            subagent_manager,
            run_options,
        )
        .await
        {
            sender.send(TuiEvent::Error(e.to_string()));
        }
    })
}

fn handle_mouse_event(mouse: MouseEvent) -> Option<InputEvent> {
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

async fn run_interactive_chat_loop(
    tui_guard: &mut tui::TuiGuard,
    app: &mut ChatApp,
    runner: InteractiveChatRunner<'_>,
) -> anyhow::Result<()> {
    let mut render_tick = tokio::time::interval(Duration::from_millis(100));
    let mut stream_flush_tick = tokio::time::interval(STREAM_CHUNK_FLUSH_INTERVAL);
    let mut needs_redraw = true;
    let mut flush_stream_before_draw = false;
    let mut pending_assistant_delta = String::new();
    let mut pending_thinking = String::new();

    loop {
        if needs_redraw {
            if flush_stream_before_draw {
                flush_stream_chunks(app, &mut pending_thinking, &mut pending_assistant_delta);
                flush_stream_before_draw = false;
            }
            tui_guard.get().draw(|f| tui::render_app(f, app))?;
            needs_redraw = false;
        }

        tokio::select! {
            input_result = handle_input_batch() => {
                let input_events = input_result?;
                let mut handled_any_input = false;
                for input_event in input_events {
                    handled_any_input = true;
                    match input_event {
                    InputEvent::Key(key_event) => {
                        handle_key_event(
                            key_event,
                            app,
                            runner.settings,
                            runner.cwd,
                            runner.event_sender,
                            || {
                                let size = tui_guard.get().size()?;
                                Ok((size.width, size.height))
                            },
                        )?;
                    }
                    InputEvent::Paste(text) => {
                        apply_paste(app, text);
                    }
                    InputEvent::ScrollUp { x, y } => {
                        let terminal_size = tui_guard.get().size()?;
                        let terminal_rect = Rect {
                            x: 0,
                            y: 0,
                            width: terminal_size.width,
                            height: terminal_size.height,
                        };
                        handle_area_scroll(app, terminal_rect, x, y, 3, 0);
                    }
                    InputEvent::ScrollDown { x, y } => {
                        let terminal_size = tui_guard.get().size()?;
                        let terminal_rect = Rect {
                            x: 0,
                            y: 0,
                            width: terminal_size.width,
                            height: terminal_size.height,
                        };
                        handle_area_scroll(
                            app,
                            terminal_rect,
                            x,
                            y,
                            0,
                            runner.scroll_down_lines,
                        );
                    }
                    InputEvent::Refresh => {
                        tui_guard.get().autoresize()?;
                        tui_guard.get().clear()?;
                    }
                    InputEvent::MouseClick { x, y } => {
                        handle_mouse_click(app, x, y, tui_guard.get());
                    }
                    InputEvent::MouseDrag { x, y } => {
                        handle_mouse_drag(app, x, y, tui_guard.get());
                    }
                    InputEvent::MouseRelease { x, y } => {
                        handle_mouse_release(app, x, y, tui_guard.get());
                    }
                    }
                }
                if handled_any_input {
                    needs_redraw = true;
                }
            }
            event = runner.event_rx.recv() => {
                if let Some(event) = event
                    && event.session_epoch == app.session_epoch()
                    && event.run_epoch == app.run_epoch()
                {
                    let mut handled_non_stream_event = false;
                    merge_or_handle_event(
                        app,
                        event.event,
                        &mut pending_thinking,
                        &mut pending_assistant_delta,
                        &mut handled_non_stream_event,
                    );

                    for _ in 0..EVENT_DRAIN_MAX {
                        let Ok(next_event) = runner.event_rx.try_recv() else {
                            break;
                        };
                        if next_event.session_epoch == app.session_epoch()
                            && next_event.run_epoch == app.run_epoch()
                        {
                            merge_or_handle_event(
                                app,
                                next_event.event,
                                &mut pending_thinking,
                                &mut pending_assistant_delta,
                                &mut handled_non_stream_event,
                            );
                        }
                    }

                    if handled_non_stream_event {
                        needs_redraw = true;
                    }
                    if pending_assistant_delta.len() >= STREAM_CHUNK_FLUSH_BYTES
                        || pending_thinking.len() >= STREAM_CHUNK_FLUSH_BYTES
                    {
                        flush_stream_before_draw = true;
                        needs_redraw = true;
                    }
                }
            }
            _ = stream_flush_tick.tick() => {
                if !pending_assistant_delta.is_empty() || !pending_thinking.is_empty() {
                    flush_stream_before_draw = true;
                    needs_redraw = true;
                }
            }
            _ = render_tick.tick() => {
                if app.on_periodic_tick() {
                    needs_redraw = true;
                }
            }
        }

        if app.should_quit {
            break;
        }
    }

    Ok(())
}

fn merge_or_handle_event(
    app: &mut ChatApp,
    event: TuiEvent,
    pending_thinking: &mut String,
    pending_assistant_delta: &mut String,
    handled_non_stream_event: &mut bool,
) {
    match event {
        TuiEvent::Thinking(chunk) => pending_thinking.push_str(&chunk),
        TuiEvent::AssistantDelta(chunk) => pending_assistant_delta.push_str(&chunk),
        other => {
            flush_stream_chunks(app, pending_thinking, pending_assistant_delta);
            app.handle_event(&other);
            *handled_non_stream_event = true;
        }
    }
}

fn flush_stream_chunks(
    app: &mut ChatApp,
    pending_thinking: &mut String,
    pending_assistant_delta: &mut String,
) {
    if !pending_thinking.is_empty() {
        let chunk = std::mem::take(pending_thinking);
        app.handle_event(&TuiEvent::Thinking(chunk));
    }
    if !pending_assistant_delta.is_empty() {
        let chunk = std::mem::take(pending_assistant_delta);
        app.handle_event(&TuiEvent::AssistantDelta(chunk));
    }
}

struct InteractiveChatRunner<'a> {
    settings: &'a Settings,
    cwd: &'a Path,
    event_sender: &'a TuiEventSender,
    event_rx: &'a mut mpsc::UnboundedReceiver<ScopedTuiEvent>,
    scroll_down_lines: usize,
}

#[derive(Clone)]
struct AgentRunOptions {
    session_id: Option<String>,
    session_title: Option<String>,
    allow_questions: bool,
}

struct AgentLoopOptions {
    subagent_manager: Option<Arc<SubagentManager>>,
    parent_task_id: Option<String>,
    depth: usize,
    session_id: Option<String>,
    session_title: Option<String>,
    session_parent_id: Option<String>,
}

fn build_session_name(cwd: &std::path::Path) -> String {
    let _ = cwd;
    "New Session".to_string()
}

fn build_model_options(settings: &Settings) -> Vec<ModelOptionView> {
    settings
        .model_refs()
        .into_iter()
        .filter_map(|model_ref| {
            settings
                .resolve_model_ref(&model_ref)
                .map(|resolved| ModelOptionView {
                    full_id: model_ref,
                    provider_name: if resolved.provider.display_name.trim().is_empty() {
                        resolved.provider_id.clone()
                    } else {
                        resolved.provider.display_name.clone()
                    },
                    model_name: if resolved.model.display_name.trim().is_empty() {
                        resolved.model_id.clone()
                    } else {
                        resolved.model.display_name.clone()
                    },
                    modality: format!(
                        "{} -> {}",
                        format_modalities(&resolved.model.modalities.input),
                        format_modalities(&resolved.model.modalities.output)
                    ),
                    max_context_size: resolved.model.limits.context,
                })
        })
        .collect()
}

fn initialize_subagent_manager(settings: Settings, cwd: PathBuf) {
    let _ = GLOBAL_SUBAGENT_MANAGER.get_or_init(|| Arc::new(build_subagent_manager(settings, cwd)));
}

fn current_subagent_manager(settings: &Settings, cwd: &Path) -> Arc<SubagentManager> {
    Arc::clone(
        GLOBAL_SUBAGENT_MANAGER
            .get_or_init(|| Arc::new(build_subagent_manager(settings.clone(), cwd.to_path_buf()))),
    )
}

fn build_subagent_manager(settings: Settings, cwd: PathBuf) -> SubagentManager {
    let enabled = settings.agent.parallel_subagents;
    let max_parallel = settings.agent.max_parallel_subagents;
    let max_depth = settings.agent.sub_agent_max_depth;
    let executor_settings = settings.clone();
    let executor: SubagentExecutor = Arc::new(move |request| {
        let settings = executor_settings.clone();
        let cwd = cwd.clone();
        Box::pin(async move {
            if !enabled {
                return SubagentExecutionResult {
                    status: SubagentStatus::Failed,
                    summary: "parallel sub-agents are disabled by configuration".to_string(),
                    error: Some("agent.parallel_subagents=false".to_string()),
                    failure_reason: Some(SubAgentFailureReason::RuntimeError),
                };
            }
            run_subagent_execution(settings, cwd, request).await
        })
    });

    SubagentManager::new(max_parallel, max_depth, executor)
}

async fn run_subagent_execution(
    settings: Settings,
    cwd: PathBuf,
    request: SubagentExecutionRequest,
) -> SubagentExecutionResult {
    let loader = match AgentLoader::new() {
        Ok(loader) => loader,
        Err(err) => {
            return SubagentExecutionResult {
                status: SubagentStatus::Failed,
                summary: "failed to initialize agent loader".to_string(),
                error: Some(err.to_string()),
                failure_reason: Some(SubAgentFailureReason::RuntimeError),
            };
        }
    };
    let registry = match loader.load_agents() {
        Ok(agents) => AgentRegistry::new(agents),
        Err(err) => {
            return SubagentExecutionResult {
                status: SubagentStatus::Failed,
                summary: "failed to load agents".to_string(),
                error: Some(err.to_string()),
                failure_reason: Some(SubAgentFailureReason::RuntimeError),
            };
        }
    };

    let Some(agent) = registry.get_agent(&request.subagent_type).cloned() else {
        return SubagentExecutionResult {
            status: SubagentStatus::Failed,
            summary: format!("unknown subagent_type: {}", request.subagent_type),
            error: None,
            failure_reason: Some(SubAgentFailureReason::RuntimeError),
        };
    };
    if agent.mode != AgentMode::Subagent {
        return SubagentExecutionResult {
            status: SubagentStatus::Failed,
            summary: format!("agent '{}' is not a subagent", agent.name),
            error: None,
            failure_reason: Some(SubAgentFailureReason::RuntimeError),
        };
    }

    let mut child_settings = settings.clone();
    child_settings.apply_agent_settings(&agent);
    child_settings.selected_agent = Some(agent.name.clone());
    let model_ref = child_settings.selected_model_ref().to_string();

    let loop_runner = match create_agent_loop(
        child_settings,
        &cwd,
        &model_ref,
        NoopEvents,
        AgentLoopOptions {
            subagent_manager: Some(current_subagent_manager(&settings, &cwd)),
            parent_task_id: Some(request.task_id.clone()),
            depth: request.depth,
            session_id: Some(request.child_session_id),
            session_title: Some(request.description),
            session_parent_id: Some(request.parent_session_id),
        },
    ) {
        Ok(loop_runner) => loop_runner,
        Err(err) => {
            return SubagentExecutionResult {
                status: SubagentStatus::Failed,
                summary: "failed to initialize sub-agent runtime".to_string(),
                error: Some(err.to_string()),
                failure_reason: Some(SubAgentFailureReason::RuntimeError),
            };
        }
    };

    match loop_runner
        .run_with_question_tool(
            Message {
                role: Role::User,
                content: request.prompt,
                attachments: Vec::new(),
                tool_call_id: None,
            },
            |_request| async {
                Ok::<crate::core::ApprovalChoice, anyhow::Error>(
                    crate::core::ApprovalChoice::AllowSession,
                )
            },
            |_questions| async {
                anyhow::bail!("question tool is not available in sub-agent mode")
            },
        )
        .await
    {
        Ok(output) => SubagentExecutionResult {
            status: SubagentStatus::Completed,
            summary: output,
            error: None,
            failure_reason: None,
        },
        Err(err) => SubagentExecutionResult {
            status: SubagentStatus::Failed,
            summary: "sub-agent execution failed".to_string(),
            error: Some(err.to_string()),
            failure_reason: Some(SubAgentFailureReason::RuntimeError),
        },
    }
}

fn format_modalities(modalities: &[crate::config::settings::ModelModalityType]) -> String {
    modalities
        .iter()
        .map(std::string::ToString::to_string)
        .collect::<Vec<_>>()
        .join(",")
}

async fn run_agent(
    settings: Settings,
    cwd: &std::path::Path,
    prompt: Message,
    model_ref: String,
    events: TuiEventSender,
    subagent_manager: Arc<SubagentManager>,
    options: AgentRunOptions,
) -> anyhow::Result<()> {
    validate_image_input_model_support(&settings, &model_ref, &prompt)?;

    let event_sender = events.clone();
    let question_event_sender = event_sender.clone();
    let approval_event_sender = event_sender.clone();
    let allow_questions = options.allow_questions;
    let parent_session_id = options.session_id.clone();
    let loop_runner = create_agent_loop(
        settings,
        cwd,
        &model_ref,
        events,
        AgentLoopOptions {
            subagent_manager: Some(Arc::clone(&subagent_manager)),
            parent_task_id: None,
            depth: 0,
            session_id: options.session_id,
            session_title: options.session_title,
            session_parent_id: None,
        },
    )?;
    loop_runner
        .run_with_question_tool(
            prompt,
            move |request| {
                let event_sender = approval_event_sender.clone();
                async move {
                    let question = approval_request_to_question_prompt(&request);
                    let (tx, rx) = oneshot::channel();
                    event_sender.send(TuiEvent::QuestionPrompt {
                        questions: vec![question],
                        responder: std::sync::Arc::new(std::sync::Mutex::new(Some(tx))),
                    });

                    let answers = rx.await.unwrap_or_else(|_| {
                        Err(anyhow::anyhow!("approval prompt was cancelled"))
                    })?;

                    Ok(
                        parse_approval_choice(&answers)
                            .unwrap_or(crate::core::ApprovalChoice::Deny),
                    )
                }
            },
            move |questions| {
                let event_sender = question_event_sender.clone();
                async move {
                    if !allow_questions {
                        anyhow::bail!("question tool is not available in this mode")
                    }
                    let (tx, rx) = oneshot::channel();
                    event_sender.send(TuiEvent::QuestionPrompt {
                        questions,
                        responder: std::sync::Arc::new(std::sync::Mutex::new(Some(tx))),
                    });
                    rx.await
                        .unwrap_or_else(|_| Err(anyhow::anyhow!("question prompt was cancelled")))
                }
            },
        )
        .await?;

    if let Some(parent_session_id) = parent_session_id.as_deref() {
        loop {
            let nodes = subagent_manager.list_for_parent(parent_session_id).await;
            event_sender.send(TuiEvent::SubagentsChanged(
                nodes.iter().map(map_subagent_node_event).collect(),
            ));

            if nodes.iter().all(|node| node.status.is_terminal()) {
                break;
            }

            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    }

    Ok(())
}

fn map_subagent_node_event(
    node: &crate::core::agent::subagent_manager::SubagentNode,
) -> tui::SubagentEventItem {
    let status = node.status.label().to_string();

    let finished_at = if node.status.is_terminal() {
        Some(node.updated_at)
    } else {
        None
    };

    tui::SubagentEventItem {
        task_id: node.task_id.clone(),
        name: node.name.clone(),
        agent_name: node.agent_name.clone(),
        status,
        prompt: node.prompt.clone(),
        depth: node.depth,
        parent_task_id: node.parent_task_id.clone(),
        started_at: node.started_at,
        finished_at,
        summary: node.summary.clone(),
        error: node.error.clone(),
    }
}

fn validate_image_input_model_support(
    settings: &Settings,
    model_ref: &str,
    prompt: &Message,
) -> anyhow::Result<()> {
    if prompt.attachments.is_empty() {
        return Ok(());
    }

    let selected = settings
        .resolve_model_ref(model_ref)
        .with_context(|| format!("unknown model reference: {model_ref}"))?;
    let supports_image_input = selected
        .model
        .modalities
        .input
        .contains(&crate::config::settings::ModelModalityType::Image);

    if supports_image_input {
        return Ok(());
    }

    anyhow::bail!(
        "Model `{model_ref}` does not support image input (input modalities: {}).",
        format_modalities(&selected.model.modalities.input)
    )
}

fn approval_request_to_question_prompt(
    request: &crate::core::ApprovalRequest,
) -> crate::core::QuestionPrompt {
    crate::core::QuestionPrompt {
        question: request.body.clone(),
        header: request.title.clone(),
        options: vec![
            crate::core::QuestionOption {
                label: "Allow Once".to_string(),
                description: "Approve this action a single time.".to_string(),
            },
            crate::core::QuestionOption {
                label: "Always Allow in Session".to_string(),
                description: "Remember this approval for the current session.".to_string(),
            },
            crate::core::QuestionOption {
                label: "Deny".to_string(),
                description: "Reject the action.".to_string(),
            },
        ],
        multiple: false,
        custom: false,
    }
}

fn parse_approval_choice(
    answers: &crate::core::QuestionAnswers,
) -> Option<crate::core::ApprovalChoice> {
    let label = answers.first()?.first()?.as_str();
    match label {
        "Allow Once" => Some(crate::core::ApprovalChoice::AllowOnce),
        "Always Allow in Session" => Some(crate::core::ApprovalChoice::AllowSession),
        "Deny" => Some(crate::core::ApprovalChoice::Deny),
        _ => None,
    }
}

pub async fn run_single_prompt(
    settings: Settings,
    cwd: &std::path::Path,
    prompt: String,
) -> anyhow::Result<String> {
    run_single_prompt_with_events(settings, cwd, prompt, NoopEvents).await
}

pub async fn run_single_prompt_with_events<E>(
    settings: Settings,
    cwd: &std::path::Path,
    prompt: String,
    events: E,
) -> anyhow::Result<String>
where
    E: AgentEvents,
{
    let default_model_ref = settings.selected_model_ref().to_string();
    let session_id = Uuid::new_v4().to_string();
    let fallback_title = fallback_session_title(&prompt);

    {
        let settings = settings.clone();
        let cwd = cwd.to_path_buf();
        let session_id = session_id.clone();
        let model_ref = default_model_ref.clone();
        let prompt = prompt.clone();
        tokio::spawn(async move {
            let generated = match generate_session_title(&settings, &model_ref, &prompt).await {
                Ok(title) => title,
                Err(_) => return,
            };

            let store =
                match SessionStore::new(&settings.session.root, &cwd, Some(&session_id), None) {
                    Ok(store) => store,
                    Err(_) => return,
                };

            let _ = store.update_title(generated);
        });
    }

    let loop_runner = create_agent_loop(
        settings.clone(),
        cwd,
        &default_model_ref,
        events,
        AgentLoopOptions {
            subagent_manager: Some(current_subagent_manager(&settings, cwd)),
            parent_task_id: None,
            depth: 0,
            session_id: Some(session_id),
            session_title: Some(fallback_title),
            session_parent_id: None,
        },
    )?;

    loop_runner
        .run_with_question_tool(
            Message {
                role: Role::User,
                content: prompt,
                attachments: Vec::new(),
                tool_call_id: None,
            },
            |request| async move {
                Ok::<crate::core::ApprovalChoice, anyhow::Error>(render::prompt_approval(&request)?)
            },
            |questions| async move { Ok(render::ask_questions(&questions)?) },
        )
        .await
}

fn create_agent_loop<E>(
    settings: Settings,
    cwd: &std::path::Path,
    model_ref: &str,
    events: E,
    options: AgentLoopOptions,
) -> anyhow::Result<
    AgentLoop<OpenAiCompatibleProvider, E, ToolRegistry, PermissionMatcher, SessionStore>,
>
where
    E: AgentEvents,
{
    let AgentLoopOptions {
        subagent_manager,
        parent_task_id,
        depth,
        session_id,
        session_title,
        session_parent_id,
    } = options;

    let selected = settings
        .resolve_model_ref(model_ref)
        .with_context(|| format!("unknown model reference: {model_ref}"))?;
    let provider = OpenAiCompatibleProvider::new(
        selected.provider.base_url.clone(),
        selected.model.id.clone(),
        selected.provider.api_key_env.clone(),
    );

    let session = match session_parent_id {
        Some(parent_session_id) => SessionStore::new_with_parent(
            &settings.session.root,
            cwd,
            session_id.as_deref(),
            session_title,
            Some(parent_session_id),
        )?,
        None => SessionStore::new(
            &settings.session.root,
            cwd,
            session_id.as_deref(),
            session_title,
        )?,
    };

    let tool_context = if let Some(manager) = subagent_manager {
        ToolRegistryContext {
            task: Some(TaskToolRuntimeContext {
                manager,
                settings: settings.clone(),
                workspace_root: cwd.to_path_buf(),
                parent_session_id: session.id.clone(),
                parent_task_id,
                depth,
            }),
        }
    } else {
        ToolRegistryContext::default()
    };

    let tool_registry = ToolRegistry::new_with_context(&settings, cwd, tool_context);
    let tool_schemas = tool_registry.schemas();
    let permissions = PermissionMatcher::new(settings.clone(), &tool_schemas);

    Ok(AgentLoop {
        provider,
        tools: tool_registry,
        approvals: permissions,
        max_steps: settings.agent.max_steps,
        model: selected.model.id.clone(),
        system_prompt: settings.agent.resolved_system_prompt(),
        session,
        events,
    })
}

use anyhow::Context;

fn handle_submitted_input(
    input: SubmittedInput,
    app: &mut ChatApp,
    settings: &Settings,
    cwd: &Path,
    event_sender: &TuiEventSender,
) {
    if input.text.starts_with('/') && input.attachments.is_empty() {
        if let Some(tui::ChatMessage::User(last)) = app.messages.last()
            && last == &input.text
        {
            app.messages.pop();
            app.mark_dirty();
        }
        handle_slash_command(input.text, app, settings, cwd, event_sender);
    } else if app.is_picking_session {
        if let Err(e) = handle_session_selection(input.text, app, settings, cwd) {
            app.messages
                .push(tui::ChatMessage::Assistant(e.to_string()));
            app.mark_dirty();
        }
        app.set_processing(false);
    } else {
        handle_chat_message(input, app, settings, cwd, event_sender);
    }
}

fn handle_slash_command(
    input: String,
    app: &mut ChatApp,
    settings: &Settings,
    cwd: &Path,
    event_sender: &TuiEventSender,
) {
    let scoped_sender = event_sender.scoped(app.session_epoch(), app.run_epoch());
    let mut parts = input.split_whitespace();
    let command = parts.next().unwrap_or_default();

    match command {
        "/new" => {
            app.start_new_session(build_session_name(cwd));
            finish_idle(app);
        }
        "/model" => {
            if let Some(model_ref) = parts.next() {
                if let Some(model) = settings.resolve_model_ref(model_ref) {
                    app.set_selected_model(model_ref);
                    finish_with_assistant(
                        app,
                        format!(
                            "Switched to {} ({} -> {}, context: {}, output: {})",
                            model_ref,
                            format_modalities(&model.model.modalities.input),
                            format_modalities(&model.model.modalities.output),
                            model.model.limits.context,
                            model.model.limits.output
                        ),
                    );
                } else {
                    finish_with_assistant(app, format!("Unknown model: {model_ref}"));
                }
            } else {
                let mut text = format!(
                    "Current model: {}\n\nAvailable models:\n",
                    app.selected_model_ref()
                );
                for option in &app.available_models {
                    text.push_str(&format!(
                        "- {} ({}, context: {} tokens)\n",
                        option.full_id, option.modality, option.max_context_size
                    ));
                }
                text.push_str("\nUse /model <provider-id/model-id> to switch.");
                finish_with_assistant(app, text);
            }
        }
        "/compact" => {
            let Some(session_id) = app.session_id.clone() else {
                finish_with_assistant(app, "No active session to compact yet.");
                return;
            };
            let model_ref = app.selected_model_ref().to_string();

            app.handle_event(&TuiEvent::CompactionStart);

            if let Ok(handle) = tokio::runtime::Handle::try_current() {
                let settings = settings.clone();
                let cwd = cwd.to_path_buf();
                let sender = scoped_sender.clone();
                handle.spawn(async move {
                    match compact_session_with_llm(settings, &cwd, &session_id, &model_ref).await {
                        Ok(summary) => sender.send(TuiEvent::CompactionDone(summary)),
                        Err(e) => sender.send(TuiEvent::Error(format!("Failed to compact: {e}"))),
                    }
                });
            } else {
                let result = tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                    .context("Failed to create runtime for compaction")
                    .and_then(|rt| {
                        rt.block_on(compact_session_with_llm(
                            settings.clone(),
                            cwd,
                            &session_id,
                            &model_ref,
                        ))
                    });

                match result {
                    Ok(summary) => {
                        app.handle_event(&TuiEvent::CompactionDone(summary));
                    }
                    Err(e) => {
                        app.handle_event(&TuiEvent::Error(format!("Failed to compact: {e}")));
                    }
                }
            }
        }
        "/quit" => {
            app.should_quit = true;
        }
        "/resume" => {
            let sessions = SessionStore::list(&settings.session.root, cwd).unwrap_or_default();
            if sessions.is_empty() {
                finish_with_assistant(app, "No previous sessions found.");
            } else {
                app.available_sessions = sessions;
                app.is_picking_session = true;

                let mut msg = String::from("Available sessions:\n");
                for (i, s) in app.available_sessions.iter().enumerate() {
                    msg.push_str(&format!("[{}] {}\n", i + 1, s.title));
                }
                msg.push_str("\nEnter number to resume:");
                finish_with_assistant(app, msg);
            }
        }
        _ => {
            finish_with_assistant(app, format!("Unknown command: {}", input));
        }
    }
}

fn finish_with_assistant(app: &mut ChatApp, message: impl Into<String>) {
    app.messages
        .push(tui::ChatMessage::Assistant(message.into()));
    finish_idle(app);
}

fn finish_idle(app: &mut ChatApp) {
    app.mark_dirty();
    app.set_processing(false);
}

async fn compact_session_with_llm(
    settings: Settings,
    cwd: &Path,
    session_id: &str,
    model_ref: &str,
) -> anyhow::Result<String> {
    let store = SessionStore::new(&settings.session.root, cwd, Some(session_id), None)
        .context("Failed to load session store")?;
    let messages = store
        .replay_messages()
        .context("Failed to replay session for compaction")?;

    if messages.is_empty() {
        return Ok("No prior context to compact yet.".to_string());
    }

    let summary = generate_compaction_summary(&settings, messages, model_ref).await?;
    store
        .append(&SessionEvent::Compact {
            id: event_id(),
            summary: summary.clone(),
        })
        .context("Failed to append compact marker")?;

    Ok(summary)
}

async fn generate_compaction_summary(
    settings: &Settings,
    messages: Vec<Message>,
    model_ref: &str,
) -> anyhow::Result<String> {
    #[cfg(test)]
    {
        let _ = settings;
        let _ = messages;
        let _ = model_ref;
        Ok("Compacted context summary for tests.".to_string())
    }

    #[cfg(not(test))]
    {
        let mut prompt_messages = Vec::with_capacity(messages.len() + 2);
        prompt_messages.push(Message {
            role: crate::core::Role::System,
            content: "You compact conversation history for an engineering assistant. Produce a concise summary that preserves requirements, decisions, constraints, open questions, and pending work items. Prefer bullet points. Do not invent details.".to_string(),
            attachments: Vec::new(),
            tool_call_id: None,
        });
        prompt_messages.extend(messages);
        prompt_messages.push(Message {
            role: crate::core::Role::User,
            content: "Compact the conversation so future turns can continue from this summary with minimal context loss.".to_string(),
            attachments: Vec::new(),
            tool_call_id: None,
        });

        let selected = settings
            .resolve_model_ref(model_ref)
            .with_context(|| format!("model is not configured: {model_ref}"))?;

        let provider = OpenAiCompatibleProvider::new(
            selected.provider.base_url.clone(),
            selected.model.id.clone(),
            selected.provider.api_key_env.clone(),
        );

        let response = crate::core::Provider::complete(
            &provider,
            crate::core::ProviderRequest {
                model: selected.model.id.clone(),
                messages: prompt_messages,
                tools: Vec::new(),
            },
        )
        .await
        .context("Compaction request failed")?;

        if !response.tool_calls.is_empty() {
            anyhow::bail!("Compaction response unexpectedly requested tools");
        }

        let summary = response.assistant_message.content.trim().to_string();
        if summary.is_empty() {
            anyhow::bail!("Compaction response was empty");
        }

        Ok(summary)
    }
}

fn handle_session_selection(
    input: String,
    app: &mut ChatApp,
    settings: &Settings,
    cwd: &Path,
) -> anyhow::Result<()> {
    let idx = input.trim().parse::<usize>().context("Invalid number.")?;

    if idx == 0 || idx > app.available_sessions.len() {
        anyhow::bail!("Invalid session index.");
    }

    let session = app.available_sessions[idx - 1].clone();
    app.bump_session_epoch();
    app.session_id = Some(session.id.clone());
    app.session_name = session.title.clone();
    app.last_context_tokens = None;
    app.is_picking_session = false;

    let store = SessionStore::new(&settings.session.root, cwd, Some(&session.id), None)
        .context("Failed to load session store")?;

    let events = store.replay_events().context("Failed to replay session")?;

    app.messages.clear();
    app.todo_items.clear();
    app.subagent_items.clear();
    let mut subagent_items_by_task: HashMap<String, tui::SubagentItemView> = HashMap::new();
    for event in events {
        match event {
            SessionEvent::Message { message, .. } => {
                let chat_msg = match message.role {
                    crate::core::Role::User => tui::ChatMessage::User(message.content),
                    crate::core::Role::Assistant => tui::ChatMessage::Assistant(message.content),
                    _ => continue,
                };
                app.messages.push(chat_msg);
            }
            SessionEvent::ToolCall { call } => {
                app.messages.push(tui::ChatMessage::ToolCall {
                    name: call.name,
                    args: call.arguments.to_string(),
                    output: None,
                    is_error: None,
                });
            }
            SessionEvent::ToolResult {
                id: _,
                is_error,
                output,
                result,
            } => {
                let pending_tool_name = app.messages.iter().rev().find_map(|msg| match msg {
                    tui::ChatMessage::ToolCall { name, output, .. } if output.is_none() => {
                        Some(name.clone())
                    }
                    _ => None,
                });
                if let Some(name) = pending_tool_name {
                    let replayed_result = result.unwrap_or_else(|| {
                        if is_error {
                            crate::tool::ToolResult::err_text("error", output)
                        } else {
                            crate::tool::ToolResult::ok_text("ok", output)
                        }
                    });
                    app.handle_event(&tui::TuiEvent::ToolEnd {
                        name,
                        result: replayed_result,
                    });
                }
            }
            SessionEvent::Thinking { content, .. } => {
                app.messages.push(tui::ChatMessage::Thinking(content));
            }
            SessionEvent::Compact { summary, .. } => {
                app.messages.push(tui::ChatMessage::Compaction(summary));
            }
            SessionEvent::SubAgentStart {
                id,
                task_id,
                name,
                parent_id,
                agent_name,
                prompt,
                depth,
                created_at,
                status,
                ..
            } => {
                let task_id = task_id.unwrap_or(id);
                subagent_items_by_task.insert(
                    task_id.clone(),
                    tui::SubagentItemView {
                        task_id,
                        name: name
                            .or_else(|| agent_name.clone())
                            .unwrap_or_else(|| "subagent".to_string()),
                        parent_task_id: parent_id,
                        agent_name: agent_name.unwrap_or_else(|| "subagent".to_string()),
                        prompt,
                        summary: None,
                        depth,
                        started_at: created_at,
                        finished_at: None,
                        status: tui::SubagentStatusView::from_lifecycle(status),
                    },
                );
            }
            SessionEvent::SubAgentResult {
                id,
                task_id,
                status,
                summary,
                output,
                ..
            } => {
                let task_id = task_id.unwrap_or(id);
                let entry = subagent_items_by_task
                    .entry(task_id.clone())
                    .or_insert_with(|| tui::SubagentItemView {
                        task_id,
                        name: "subagent".to_string(),
                        parent_task_id: None,
                        agent_name: "subagent".to_string(),
                        prompt: String::new(),
                        summary: None,
                        depth: 0,
                        started_at: 0,
                        finished_at: None,
                        status: tui::SubagentStatusView::Running,
                    });
                entry.status = tui::SubagentStatusView::from_lifecycle(status);
                if entry.status.is_terminal() {
                    entry.finished_at = Some(entry.started_at);
                }
                entry.summary = if let Some(summary) = summary {
                    Some(summary)
                } else if output.trim().is_empty() {
                    None
                } else {
                    Some(output)
                };
            }
            _ => {}
        }
    }
    app.subagent_items = subagent_items_by_task.into_values().collect();
    for item in &mut app.subagent_items {
        if item.status.is_active() {
            item.status = tui::SubagentStatusView::Failed;
            if item.summary.is_none() {
                item.summary = Some("interrupted_by_restart".to_string());
            }
        }
    }
    app.mark_dirty();

    Ok(())
}

fn handle_chat_message(
    input: SubmittedInput,
    app: &mut ChatApp,
    settings: &Settings,
    cwd: &Path,
    event_sender: &TuiEventSender,
) {
    if !input.text.is_empty() || !input.attachments.is_empty() {
        // Ensure any run-epoch bump from replacing an existing task happens
        // before we scope events for the new run.
        app.cancel_agent_task();

        let scoped_sender = event_sender.scoped(app.session_epoch(), app.run_epoch());
        let session_id = app.session_id.clone();
        let session_title = if session_id.is_none() {
            Some(fallback_session_title(&input.text))
        } else {
            None
        };

        let current_session_id = session_id.unwrap_or_else(|| Uuid::new_v4().to_string());
        if app.session_id.is_none() {
            app.session_id = Some(current_session_id.clone());
            if let Some(t) = &session_title {
                app.session_name = t.clone();
            }
            if !input.text.trim().is_empty() {
                spawn_session_title_generation_task(
                    settings,
                    cwd,
                    current_session_id.clone(),
                    app.selected_model_ref().to_string(),
                    input.text.clone(),
                    &scoped_sender,
                );
            }
        }

        let message = Message {
            role: crate::core::Role::User,
            content: input.text,
            attachments: input.attachments,
            tool_call_id: None,
        };

        let subagent_manager = current_subagent_manager(settings, cwd);
        let handle = spawn_agent_task(
            settings,
            cwd,
            message,
            app.selected_model_ref().to_string(),
            &scoped_sender,
            subagent_manager,
            AgentRunOptions {
                session_id: Some(current_session_id),
                session_title,
                allow_questions: true,
            },
        );
        app.set_agent_task(handle);
    } else {
        app.set_processing(false);
    }
}

fn fallback_session_title(prompt: &str) -> String {
    let trimmed = prompt.trim();
    if trimmed.is_empty() {
        return "Image input".to_string();
    }

    trimmed
        .split_whitespace()
        .take(12)
        .collect::<Vec<_>>()
        .join(" ")
}

fn normalize_session_title(raw: &str, fallback: &str) -> String {
    let cleaned = raw
        .lines()
        .next()
        .unwrap_or_default()
        .trim()
        .trim_matches('"')
        .trim_matches('`')
        .split_whitespace()
        .take(12)
        .collect::<Vec<_>>()
        .join(" ");

    if cleaned.is_empty() {
        fallback.to_string()
    } else {
        cleaned
    }
}

fn spawn_session_title_generation_task(
    settings: &Settings,
    cwd: &Path,
    session_id: String,
    model_ref: String,
    prompt: String,
    event_sender: &TuiEventSender,
) {
    let settings = settings.clone();
    let cwd = cwd.to_path_buf();
    let event_sender = event_sender.clone();
    tokio::spawn(async move {
        let fallback = fallback_session_title(&prompt);
        let generated = match generate_session_title(&settings, &model_ref, &prompt).await {
            Ok(title) => title,
            Err(_) => return,
        };

        let store = match SessionStore::new(&settings.session.root, &cwd, Some(&session_id), None) {
            Ok(store) => store,
            Err(_) => return,
        };

        let title = normalize_session_title(&generated, &fallback);
        if store.update_title(title.clone()).is_ok() {
            event_sender.send(TuiEvent::SessionTitle(title));
        }
    });
}

async fn generate_session_title(
    settings: &Settings,
    model_ref: &str,
    prompt: &str,
) -> anyhow::Result<String> {
    #[cfg(test)]
    {
        let _ = settings;
        let _ = model_ref;
        Ok(normalize_session_title(
            "Generated test title",
            &fallback_session_title(prompt),
        ))
    }

    #[cfg(not(test))]
    {
        let selected = settings
            .resolve_model_ref(model_ref)
            .with_context(|| format!("model is not configured: {model_ref}"))?;

        let provider = OpenAiCompatibleProvider::new(
            selected.provider.base_url.clone(),
            selected.model.id.clone(),
            selected.provider.api_key_env.clone(),
        );

        let request = crate::core::ProviderRequest {
            model: selected.model.id.clone(),
            messages: vec![
                Message {
                    role: crate::core::Role::System,
                    content: "Generate a concise session title for this prompt. Return only the title, no punctuation wrappers, and keep it to 12 words or fewer.".to_string(),
                    attachments: Vec::new(),
                    tool_call_id: None,
                },
                Message {
                    role: crate::core::Role::User,
                    content: prompt.to_string(),
                    attachments: Vec::new(),
                    tool_call_id: None,
                },
            ],
            tools: Vec::new(),
        };

        let mut last_error: Option<anyhow::Error> = None;
        for attempt in 1..=3 {
            if attempt > 1 {
                tokio::time::sleep(Duration::from_millis(350 * attempt as u64)).await;
            }

            match crate::core::Provider::complete_stream(&provider, request.clone(), |_| {}).await {
                Ok(response) => {
                    if !response.tool_calls.is_empty() {
                        anyhow::bail!("Session title response unexpectedly requested tools");
                    }

                    let fallback = fallback_session_title(prompt);
                    return Ok(normalize_session_title(
                        &response.assistant_message.content,
                        &fallback,
                    ));
                }
                Err(err) => {
                    last_error =
                        Some(err.context(format!("title generation attempt {attempt}/3 failed")));
                }
            }
        }

        let err = last_error.unwrap_or_else(|| anyhow::anyhow!("unknown title request failure"));
        Err(err).context("Session title request failed")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::settings::{
        AgentSettings, ModelLimits, ModelMetadata, ModelModalities, ModelModalityType,
        ModelSettings, ProviderConfig, SessionSettings,
    };
    use crate::core::{Message, Role};
    use crossterm::event::{KeyEvent, KeyModifiers, MouseEvent, MouseEventKind};
    use std::collections::BTreeMap;
    use tempfile::tempdir;

    fn create_dummy_settings(root: &Path) -> Settings {
        Settings {
            models: ModelSettings {
                default: "test/test-model".to_string(),
            },
            providers: BTreeMap::from([(
                "test".to_string(),
                ProviderConfig {
                    display_name: "Test Provider".to_string(),
                    base_url: "http://localhost:1234".to_string(),
                    api_key_env: "TEST_KEY".to_string(),
                    models: BTreeMap::from([(
                        "test-model".to_string(),
                        ModelMetadata {
                            id: "provider-test-model".to_string(),
                            display_name: "Test Model".to_string(),
                            modalities: ModelModalities {
                                input: vec![ModelModalityType::Text],
                                output: vec![ModelModalityType::Text],
                            },
                            limits: ModelLimits {
                                context: 64_000,
                                output: 8_000,
                            },
                        },
                    )]),
                },
            )]),
            agent: AgentSettings {
                max_steps: 10,
                sub_agent_max_depth: 2,
                parallel_subagents: false,
                max_parallel_subagents: 2,
                system_prompt: None,
            },
            session: SessionSettings {
                root: root.to_path_buf(),
            },
            tools: Default::default(),
            permission: Default::default(),
            selected_agent: None,
            agents: BTreeMap::new(),
        }
    }

    #[test]
    fn test_resume_clears_processing() {
        let temp_dir = tempdir().unwrap();
        let settings = create_dummy_settings(temp_dir.path());
        let cwd = temp_dir.path();

        // Create a dummy session
        let session_id = "test-session-id";
        let _store = SessionStore::new(
            &settings.session.root,
            cwd,
            Some(session_id),
            Some("Test Session".to_string()),
        )
        .unwrap();

        // Setup ChatApp
        let mut app = ChatApp::new("Session".to_string(), cwd);
        let (tx, _rx) = mpsc::unbounded_channel();
        let event_sender = TuiEventSender::new(tx);

        // Simulate typing "/resume"
        app.set_input("/resume".to_string());
        // verify submit_input sets processing to true
        let input = app.submit_input();
        assert!(app.is_processing);

        handle_submitted_input(input, &mut app, &settings, cwd, &event_sender);

        // processing should be false after listing sessions
        assert!(
            !app.is_processing,
            "Processing should be cleared after /resume lists sessions"
        );
        assert!(app.is_picking_session);

        // Simulate picking session "1"
        app.set_input("1".to_string());
        let input = app.submit_input();
        assert!(app.is_processing);

        handle_submitted_input(input, &mut app, &settings, cwd, &event_sender);

        // processing should be false after picking session
        assert!(
            !app.is_processing,
            "Processing should be cleared after picking session"
        );
        assert!(!app.is_picking_session);
        // The session ID might not match if listing logic uses UUIDs or if index logic is tricky.
        // But we provided title "Test Session", so it should be listed.
        // Let's verify session_id is SOME value, and name is correct.
        assert_eq!(app.session_name, "Test Session");
    }

    #[test]
    fn test_session_selection_restores_todos_from_todo_write_and_replaces_stale_items() {
        let temp_dir = tempdir().unwrap();
        let settings = create_dummy_settings(temp_dir.path());
        let cwd = temp_dir.path();

        let session_id = "todo-session-id";
        let store = SessionStore::new(
            &settings.session.root,
            cwd,
            Some(session_id),
            Some("Todo Session".to_string()),
        )
        .unwrap();

        store
            .append(&SessionEvent::ToolCall {
                call: crate::core::ToolCall {
                    id: "call-1".to_string(),
                    name: "todo_write".to_string(),
                    arguments: serde_json::json!({"todos": []}),
                },
            })
            .unwrap();
        store
            .append(&SessionEvent::ToolResult {
                id: "call-1".to_string(),
                is_error: false,
                output: "".to_string(),
                result: Some(crate::tool::ToolResult::ok_json_typed(
                    "todo list updated",
                    "application/vnd.hh.todo+json",
                    serde_json::json!({
                        "todos": [
                            {"content": "Resume pending", "status": "pending", "priority": "medium"},
                            {"content": "Resume done", "status": "completed", "priority": "high"}
                        ],
                        "counts": {"total": 2, "pending": 1, "in_progress": 0, "completed": 1, "cancelled": 0}
                    }),
                )),
            })
            .unwrap();

        let mut app = ChatApp::new("Session".to_string(), cwd);
        app.handle_event(&TuiEvent::ToolStart {
            name: "todo_write".to_string(),
            args: serde_json::json!({"todos": []}),
        });
        app.handle_event(&TuiEvent::ToolEnd {
            name: "todo_write".to_string(),
            result: crate::tool::ToolResult::ok_json_typed(
                "todo list updated",
                "application/vnd.hh.todo+json",
                serde_json::json!({
                    "todos": [
                        {"content": "Stale item", "status": "pending", "priority": "low"}
                    ],
                    "counts": {"total": 1, "pending": 1, "in_progress": 0, "completed": 0, "cancelled": 0}
                }),
            ),
        });

        app.available_sessions = vec![crate::session::SessionMetadata {
            id: session_id.to_string(),
            title: "Todo Session".to_string(),
            created_at: 0,
            last_updated_at: 0,
            parent_session_id: None,
        }];
        app.is_picking_session = true;

        handle_session_selection("1".to_string(), &mut app, &settings, cwd).unwrap();

        let backend = ratatui::backend::TestBackend::new(120, 25);
        let mut terminal = ratatui::Terminal::new(backend).expect("terminal");
        terminal
            .draw(|frame| tui::render_app(frame, &app))
            .expect("draw app");
        let full_text = terminal
            .backend()
            .buffer()
            .content()
            .iter()
            .map(|cell| cell.symbol())
            .collect::<String>();

        assert!(full_text.contains("TODO"));
        assert!(full_text.contains("1 / 2 done"));
        assert!(full_text.contains("[ ] Resume pending"));
        assert!(full_text.contains("[x] Resume done"));
        assert!(!full_text.contains("Stale item"));
    }

    #[test]
    fn test_new_starts_fresh_session() {
        let temp_dir = tempdir().unwrap();
        let settings = create_dummy_settings(temp_dir.path());
        let cwd = temp_dir.path();
        let (tx, _rx) = mpsc::unbounded_channel();
        let event_sender = TuiEventSender::new(tx);

        let mut app = ChatApp::new("Session".to_string(), cwd);
        app.session_id = Some("existing-session".to_string());
        app.session_name = "Existing Session".to_string();
        app.messages
            .push(tui::ChatMessage::Assistant("previous context".to_string()));

        app.set_input("/new".to_string());
        let input = app.submit_input();
        handle_submitted_input(input, &mut app, &settings, cwd, &event_sender);

        assert!(!app.is_processing);
        assert!(app.session_id.is_none());
        assert_eq!(app.session_name, build_session_name(cwd));
        assert!(app.messages.is_empty());
    }

    #[test]
    fn test_new_session_ignores_stale_scoped_events() {
        let temp_dir = tempdir().unwrap();
        let cwd = temp_dir.path();
        let mut app = ChatApp::new("Session".to_string(), cwd);
        let (tx, mut rx) = mpsc::unbounded_channel();
        let event_sender = TuiEventSender::new(tx);

        let old_scope_sender = event_sender.scoped(app.session_epoch(), app.run_epoch());
        app.start_new_session("New Session".to_string());

        old_scope_sender.send(TuiEvent::AssistantDelta("stale".to_string()));
        let stale_event = rx.blocking_recv().unwrap();
        if stale_event.session_epoch == app.session_epoch()
            && stale_event.run_epoch == app.run_epoch()
        {
            app.handle_event(&stale_event.event);
        }
        assert!(app.messages.is_empty());

        let current_scope_sender = event_sender.scoped(app.session_epoch(), app.run_epoch());
        current_scope_sender.send(TuiEvent::AssistantDelta("fresh".to_string()));
        let fresh_event = rx.blocking_recv().unwrap();
        if fresh_event.session_epoch == app.session_epoch()
            && fresh_event.run_epoch == app.run_epoch()
        {
            app.handle_event(&fresh_event.event);
        }

        assert!(matches!(
            app.messages.first(),
            Some(tui::ChatMessage::Assistant(text)) if text == "fresh"
        ));
    }

    #[test]
    fn test_set_agent_task_without_existing_task_keeps_run_epoch_and_allows_events() {
        let temp_dir = tempdir().unwrap();
        let cwd = temp_dir.path();
        let mut app = ChatApp::new("Session".to_string(), cwd);
        let (tx, mut rx) = mpsc::unbounded_channel();
        let event_sender = TuiEventSender::new(tx);
        app.set_processing(true);

        let initial_run_epoch = app.run_epoch();
        let scoped_sender = event_sender.scoped(app.session_epoch(), app.run_epoch());

        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");
        #[allow(clippy::async_yields_async)]
        let handle = runtime.block_on(async { tokio::spawn(async {}) });
        app.set_agent_task(handle);

        assert_eq!(app.run_epoch(), initial_run_epoch);

        scoped_sender.send(TuiEvent::AssistantDone);
        let event = rx.blocking_recv().expect("event");
        if event.session_epoch == app.session_epoch() && event.run_epoch == app.run_epoch() {
            app.handle_event(&event.event);
        }

        assert!(!app.is_processing);
        app.cancel_agent_task();
    }

    #[test]
    fn test_compact_appends_marker_and_clears_replayed_context() {
        let temp_dir = tempdir().unwrap();
        let settings = create_dummy_settings(temp_dir.path());
        let cwd = temp_dir.path();
        let (tx, _rx) = mpsc::unbounded_channel();
        let event_sender = TuiEventSender::new(tx);

        let session_id = "compact-session-id";
        let store = SessionStore::new(
            &settings.session.root,
            cwd,
            Some(session_id),
            Some("Compact Session".to_string()),
        )
        .unwrap();
        store
            .append(&SessionEvent::Message {
                id: event_id(),
                message: Message {
                    role: Role::User,
                    content: "hello".to_string(),
                    attachments: Vec::new(),
                    tool_call_id: None,
                },
            })
            .unwrap();

        let mut app = ChatApp::new("Session".to_string(), cwd);
        app.session_id = Some(session_id.to_string());
        app.session_name = "Compact Session".to_string();
        app.messages
            .push(tui::ChatMessage::Assistant("previous context".to_string()));

        app.set_input("/compact".to_string());
        let input = app.submit_input();
        handle_submitted_input(input, &mut app, &settings, cwd, &event_sender);

        assert!(!app.is_processing);
        assert_eq!(app.messages.len(), 2);
        assert!(matches!(
            app.messages[0],
            tui::ChatMessage::Assistant(ref text) if text == "previous context"
        ));
        assert!(matches!(
            app.messages[1],
            tui::ChatMessage::Compaction(ref text)
                if text == "Compacted context summary for tests."
        ));

        let store = SessionStore::new(&settings.session.root, cwd, Some(session_id), None).unwrap();
        let replayed_events = store.replay_events().unwrap();
        assert_eq!(replayed_events.len(), 2);
        assert!(matches!(
            replayed_events[1],
            SessionEvent::Compact { ref summary, .. } if summary == "Compacted context summary for tests."
        ));

        let replayed_messages = store.replay_messages().unwrap();
        assert_eq!(replayed_messages.len(), 1);
        assert_eq!(
            replayed_messages[0].content,
            "Compacted context summary for tests."
        );
    }

    #[test]
    fn test_esc_requires_two_presses_to_interrupt_processing() {
        let temp_dir = tempdir().unwrap();
        let settings = create_dummy_settings(temp_dir.path());
        let cwd = temp_dir.path();
        let (tx, _rx) = mpsc::unbounded_channel();
        let event_sender = TuiEventSender::new(tx);
        let mut app = ChatApp::new("Session".to_string(), cwd);
        app.set_processing(true);

        handle_key_event(
            KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE),
            &mut app,
            &settings,
            cwd,
            &event_sender,
            || Ok((120, 40)),
        )
        .unwrap();

        assert!(app.is_processing);
        assert!(app.should_interrupt_on_esc());
        assert_eq!(app.processing_interrupt_hint(), "esc again to interrupt");

        handle_key_event(
            KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE),
            &mut app,
            &settings,
            cwd,
            &event_sender,
            || Ok((120, 40)),
        )
        .unwrap();

        assert!(!app.is_processing);
        assert!(!app.should_interrupt_on_esc());
        assert_eq!(app.processing_interrupt_hint(), "esc interrupt");
    }

    #[test]
    fn test_non_esc_key_clears_pending_interrupt_confirmation() {
        let temp_dir = tempdir().unwrap();
        let settings = create_dummy_settings(temp_dir.path());
        let cwd = temp_dir.path();
        let (tx, _rx) = mpsc::unbounded_channel();
        let event_sender = TuiEventSender::new(tx);
        let mut app = ChatApp::new("Session".to_string(), cwd);
        app.set_processing(true);

        handle_key_event(
            KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE),
            &mut app,
            &settings,
            cwd,
            &event_sender,
            || Ok((120, 40)),
        )
        .unwrap();
        assert!(app.should_interrupt_on_esc());

        handle_key_event(
            KeyEvent::new(KeyCode::Left, KeyModifiers::NONE),
            &mut app,
            &settings,
            cwd,
            &event_sender,
            || Ok((120, 40)),
        )
        .unwrap();

        assert!(app.is_processing);
        assert!(!app.should_interrupt_on_esc());
        assert_eq!(app.processing_interrupt_hint(), "esc interrupt");

        handle_key_event(
            KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE),
            &mut app,
            &settings,
            cwd,
            &event_sender,
            || Ok((120, 40)),
        )
        .unwrap();

        assert!(app.is_processing);
        assert!(app.should_interrupt_on_esc());
        assert_eq!(app.processing_interrupt_hint(), "esc again to interrupt");
    }

    #[test]
    fn test_cancelled_run_ignores_queued_events_from_previous_run_epoch() {
        let temp_dir = tempdir().unwrap();
        let settings = create_dummy_settings(temp_dir.path());
        let cwd = temp_dir.path();
        let (tx, mut rx) = mpsc::unbounded_channel();
        let event_sender = TuiEventSender::new(tx);
        let mut app = ChatApp::new("Session".to_string(), cwd);
        app.set_processing(true);

        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");
        #[allow(clippy::async_yields_async)]
        let handle = runtime.block_on(async {
            tokio::spawn(async {
                tokio::time::sleep(std::time::Duration::from_millis(200)).await;
            })
        });
        app.set_agent_task(handle);

        let old_scope_sender = event_sender.scoped(app.session_epoch(), app.run_epoch());

        handle_key_event(
            KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE),
            &mut app,
            &settings,
            cwd,
            &event_sender,
            || Ok((120, 40)),
        )
        .unwrap();
        handle_key_event(
            KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE),
            &mut app,
            &settings,
            cwd,
            &event_sender,
            || Ok((120, 40)),
        )
        .unwrap();

        assert!(!app.is_processing);

        old_scope_sender.send(TuiEvent::AssistantDelta("stale-stream".to_string()));
        let stale_event = rx.blocking_recv().unwrap();
        if stale_event.session_epoch == app.session_epoch()
            && stale_event.run_epoch == app.run_epoch()
        {
            app.handle_event(&stale_event.event);
        }

        assert!(!app.messages.iter().any(
            |message| matches!(message, tui::ChatMessage::Assistant(text) if text.contains("stale-stream"))
        ));

        app.cancel_agent_task();
    }

    #[test]
    fn test_replacing_finished_task_scopes_events_to_new_run_epoch() {
        let temp_dir = tempdir().unwrap();
        let settings = create_dummy_settings(temp_dir.path());
        let cwd = temp_dir.path();
        let (tx, mut rx) = mpsc::unbounded_channel();
        let event_sender = TuiEventSender::new(tx);
        let mut app = ChatApp::new("Session".to_string(), cwd);

        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        #[allow(clippy::async_yields_async)]
        let first_handle = runtime.block_on(async { tokio::spawn(async {}) });
        app.set_agent_task(first_handle);
        app.set_processing(true);

        let submitted = SubmittedInput {
            text: "follow-up".to_string(),
            attachments: vec![crate::core::MessageAttachment::Image {
                media_type: "image/png".to_string(),
                data_base64: "aGVsbG8=".to_string(),
            }],
        };

        let _enter = runtime.enter();
        handle_chat_message(submitted, &mut app, &settings, cwd, &event_sender);
        drop(_enter);

        runtime.block_on(async {
            tokio::time::sleep(std::time::Duration::from_millis(60)).await;
        });

        while let Ok(event) = rx.try_recv() {
            if event.session_epoch == app.session_epoch() && event.run_epoch == app.run_epoch() {
                app.handle_event(&event.event);
            }
        }

        assert!(
            app.messages
                .iter()
                .any(|message| matches!(message, tui::ChatMessage::Error(_))),
            "expected an error event from the newly started run"
        );
        assert!(
            !app.is_processing,
            "processing should stop when the run emits a scoped error event"
        );

        app.cancel_agent_task();
    }

    #[test]
    fn test_shift_enter_inserts_newline_without_submitting() {
        let temp_dir = tempdir().unwrap();
        let settings = create_dummy_settings(temp_dir.path());
        let cwd = temp_dir.path();
        let (tx, _rx) = mpsc::unbounded_channel();
        let event_sender = TuiEventSender::new(tx);
        let mut app = ChatApp::new("Session".to_string(), cwd);
        app.set_input("hello".to_string());

        handle_key_event(
            KeyEvent::new(KeyCode::Enter, KeyModifiers::SHIFT),
            &mut app,
            &settings,
            cwd,
            &event_sender,
            || Ok((120, 40)),
        )
        .unwrap();

        assert_eq!(app.input, "hello\n");
        assert!(app.messages.is_empty());
        assert!(!app.is_processing);
    }

    #[test]
    fn test_shift_enter_press_followed_by_release_does_not_submit() {
        let temp_dir = tempdir().unwrap();
        let settings = create_dummy_settings(temp_dir.path());
        let cwd = temp_dir.path();
        let (tx, _rx) = mpsc::unbounded_channel();
        let event_sender = TuiEventSender::new(tx);
        let mut app = ChatApp::new("Session".to_string(), cwd);
        app.set_input("hello".to_string());

        handle_key_event(
            KeyEvent::new_with_kind(KeyCode::Enter, KeyModifiers::SHIFT, KeyEventKind::Press),
            &mut app,
            &settings,
            cwd,
            &event_sender,
            || Ok((120, 40)),
        )
        .unwrap();

        handle_key_event(
            KeyEvent::new_with_kind(KeyCode::Enter, KeyModifiers::NONE, KeyEventKind::Release),
            &mut app,
            &settings,
            cwd,
            &event_sender,
            || Ok((120, 40)),
        )
        .unwrap();

        assert_eq!(app.input, "hello\n");
        assert!(app.messages.is_empty());
        assert!(!app.is_processing);
    }

    #[test]
    fn test_ctrl_c_clears_non_empty_input() {
        let temp_dir = tempdir().unwrap();
        let settings = create_dummy_settings(temp_dir.path());
        let cwd = temp_dir.path();
        let (tx, _rx) = mpsc::unbounded_channel();
        let event_sender = TuiEventSender::new(tx);
        let mut app = ChatApp::new("Session".to_string(), cwd);
        app.set_input("hello".to_string());

        handle_key_event(
            KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL),
            &mut app,
            &settings,
            cwd,
            &event_sender,
            || Ok((120, 40)),
        )
        .unwrap();

        assert!(app.input.is_empty());
        assert_eq!(app.cursor, 0);
        assert!(!app.should_quit);
    }

    #[test]
    fn test_ctrl_c_quits_when_input_is_empty() {
        let temp_dir = tempdir().unwrap();
        let settings = create_dummy_settings(temp_dir.path());
        let cwd = temp_dir.path();
        let (tx, _rx) = mpsc::unbounded_channel();
        let event_sender = TuiEventSender::new(tx);
        let mut app = ChatApp::new("Session".to_string(), cwd);

        handle_key_event(
            KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL),
            &mut app,
            &settings,
            cwd,
            &event_sender,
            || Ok((120, 40)),
        )
        .unwrap();

        assert!(app.should_quit);
    }

    #[test]
    fn test_multiline_cursor_shortcuts_ctrl_and_vertical_arrows() {
        let temp_dir = tempdir().unwrap();
        let settings = create_dummy_settings(temp_dir.path());
        let cwd = temp_dir.path();
        let (tx, _rx) = mpsc::unbounded_channel();
        let event_sender = TuiEventSender::new(tx);
        let mut app = ChatApp::new("Session".to_string(), cwd);
        app.set_input("abc\ndefg\nxy".to_string());

        handle_key_event(
            KeyEvent::new(KeyCode::Char('a'), KeyModifiers::CONTROL),
            &mut app,
            &settings,
            cwd,
            &event_sender,
            || Ok((120, 40)),
        )
        .unwrap();
        assert_eq!(app.cursor, 9);

        handle_key_event(
            KeyEvent::new(KeyCode::Up, KeyModifiers::NONE),
            &mut app,
            &settings,
            cwd,
            &event_sender,
            || Ok((120, 40)),
        )
        .unwrap();
        assert_eq!(app.cursor, 4);

        handle_key_event(
            KeyEvent::new(KeyCode::Down, KeyModifiers::NONE),
            &mut app,
            &settings,
            cwd,
            &event_sender,
            || Ok((120, 40)),
        )
        .unwrap();
        assert_eq!(app.cursor, 9);

        handle_key_event(
            KeyEvent::new(KeyCode::Char('e'), KeyModifiers::CONTROL),
            &mut app,
            &settings,
            cwd,
            &event_sender,
            || Ok((120, 40)),
        )
        .unwrap();
        assert_eq!(app.cursor, 11);
    }

    #[test]
    fn test_ctrl_e_and_ctrl_a_can_cross_line_edges() {
        let temp_dir = tempdir().unwrap();
        let settings = create_dummy_settings(temp_dir.path());
        let cwd = temp_dir.path();
        let (tx, _rx) = mpsc::unbounded_channel();
        let event_sender = TuiEventSender::new(tx);
        let mut app = ChatApp::new("Session".to_string(), cwd);
        app.set_input("ab\ncd\nef".to_string());

        // End of first line.
        app.cursor = 2;

        handle_key_event(
            KeyEvent::new(KeyCode::Char('e'), KeyModifiers::CONTROL),
            &mut app,
            &settings,
            cwd,
            &event_sender,
            || Ok((120, 40)),
        )
        .unwrap();
        assert_eq!(app.cursor, 5);

        // End of second line should jump to end of third line.
        handle_key_event(
            KeyEvent::new(KeyCode::Char('e'), KeyModifiers::CONTROL),
            &mut app,
            &settings,
            cwd,
            &event_sender,
            || Ok((120, 40)),
        )
        .unwrap();
        assert_eq!(app.cursor, 8);

        // On last line end, Ctrl+E stays there.
        handle_key_event(
            KeyEvent::new(KeyCode::Char('e'), KeyModifiers::CONTROL),
            &mut app,
            &settings,
            cwd,
            &event_sender,
            || Ok((120, 40)),
        )
        .unwrap();
        assert_eq!(app.cursor, 8);

        // Ctrl+A at line end moves to that line's start.
        handle_key_event(
            KeyEvent::new(KeyCode::Char('a'), KeyModifiers::CONTROL),
            &mut app,
            &settings,
            cwd,
            &event_sender,
            || Ok((120, 40)),
        )
        .unwrap();
        assert_eq!(app.cursor, 6);

        // Ctrl+A at line start jumps to previous line start.
        handle_key_event(
            KeyEvent::new(KeyCode::Char('a'), KeyModifiers::CONTROL),
            &mut app,
            &settings,
            cwd,
            &event_sender,
            || Ok((120, 40)),
        )
        .unwrap();
        assert_eq!(app.cursor, 3);

        handle_key_event(
            KeyEvent::new(KeyCode::Char('a'), KeyModifiers::CONTROL),
            &mut app,
            &settings,
            cwd,
            &event_sender,
            || Ok((120, 40)),
        )
        .unwrap();
        assert_eq!(app.cursor, 0);
    }

    #[test]
    fn test_left_and_right_move_cursor_across_newline() {
        let temp_dir = tempdir().unwrap();
        let settings = create_dummy_settings(temp_dir.path());
        let cwd = temp_dir.path();
        let (tx, _rx) = mpsc::unbounded_channel();
        let event_sender = TuiEventSender::new(tx);
        let mut app = ChatApp::new("Session".to_string(), cwd);
        app.set_input("ab\ncd".to_string());
        app.cursor = 2;

        handle_key_event(
            KeyEvent::new(KeyCode::Right, KeyModifiers::NONE),
            &mut app,
            &settings,
            cwd,
            &event_sender,
            || Ok((120, 40)),
        )
        .unwrap();
        assert_eq!(app.cursor, 3);

        handle_key_event(
            KeyEvent::new(KeyCode::Left, KeyModifiers::NONE),
            &mut app,
            &settings,
            cwd,
            &event_sender,
            || Ok((120, 40)),
        )
        .unwrap();
        assert_eq!(app.cursor, 2);

        app.cursor = 0;
        handle_key_event(
            KeyEvent::new(KeyCode::Left, KeyModifiers::NONE),
            &mut app,
            &settings,
            cwd,
            &event_sender,
            || Ok((120, 40)),
        )
        .unwrap();
        assert_eq!(app.cursor, 0);

        app.cursor = app.input.len();
        handle_key_event(
            KeyEvent::new(KeyCode::Right, KeyModifiers::NONE),
            &mut app,
            &settings,
            cwd,
            &event_sender,
            || Ok((120, 40)),
        )
        .unwrap();
        assert_eq!(app.cursor, app.input.len());
    }

    #[test]
    fn test_paste_transforms_single_image_path_into_attachment() {
        let temp_dir = tempdir().unwrap();
        let image_path = temp_dir.path().join("example.png");
        std::fs::write(&image_path, [1u8, 2, 3, 4]).unwrap();

        let prepared = prepare_paste(image_path.to_string_lossy().as_ref());
        assert_eq!(prepared.insert_text, "[pasted image: example.png]");
        assert_eq!(prepared.attachments.len(), 1);
    }

    #[test]
    fn test_paste_transforms_shell_escaped_image_path_into_attachment() {
        let temp_dir = tempdir().unwrap();
        let image_path = temp_dir.path().join("my image.png");
        std::fs::write(&image_path, [1u8, 2, 3, 4]).unwrap();
        let escaped = image_path.to_string_lossy().replace(' ', "\\ ");

        let prepared = prepare_paste(&escaped);
        assert_eq!(prepared.insert_text, "[pasted image: my image.png]");
        assert_eq!(prepared.attachments.len(), 1);
    }

    #[test]
    fn test_paste_transforms_file_url_image_path_into_attachment() {
        let temp_dir = tempdir().unwrap();
        let image_path = temp_dir.path().join("my image.jpeg");
        std::fs::write(&image_path, [1u8, 2, 3, 4]).unwrap();
        let file_url = format!(
            "file://{}",
            image_path.to_string_lossy().replace(' ', "%20")
        );

        let prepared = prepare_paste(&file_url);
        assert_eq!(prepared.insert_text, "[pasted image: my image.jpeg]");
        assert_eq!(prepared.attachments.len(), 1);
    }

    #[test]
    fn test_paste_leaves_plain_text_unchanged() {
        let prepared = prepare_paste("hello\nworld");
        assert_eq!(prepared.insert_text, "hello\nworld");
        assert!(prepared.attachments.is_empty());
    }

    #[test]
    fn test_apply_paste_inserts_content_at_cursor() {
        let temp_dir = tempdir().unwrap();
        let cwd = temp_dir.path();
        let mut app = ChatApp::new("Session".to_string(), cwd);
        app.set_input("abcXYZ".to_string());
        app.cursor = 3;

        let image_path = temp_dir.path().join("shot.png");
        std::fs::write(&image_path, [1u8, 2, 3, 4]).unwrap();

        apply_paste(&mut app, image_path.to_string_lossy().to_string());

        assert_eq!(app.input, "abc[pasted image: shot.png]XYZ");
        assert_eq!(app.pending_attachments.len(), 1);
    }

    #[test]
    fn test_cmd_v_does_not_insert_literal_v() {
        let temp_dir = tempdir().unwrap();
        let settings = create_dummy_settings(temp_dir.path());
        let cwd = temp_dir.path();
        let (tx, _rx) = mpsc::unbounded_channel();
        let event_sender = TuiEventSender::new(tx);
        let mut app = ChatApp::new("Session".to_string(), cwd);
        app.set_input("abc".to_string());

        handle_key_event(
            KeyEvent::new(KeyCode::Char('v'), KeyModifiers::SUPER),
            &mut app,
            &settings,
            cwd,
            &event_sender,
            || Ok((120, 40)),
        )
        .unwrap();

        assert_ne!(app.input, "abcv");
    }

    #[test]
    fn test_mouse_wheel_event_keeps_cursor_coordinates() {
        let event = MouseEvent {
            kind: MouseEventKind::ScrollDown,
            column: 77,
            row: 14,
            modifiers: KeyModifiers::NONE,
        };

        let translated = handle_mouse_event(event);
        assert!(matches!(
            translated,
            Some(InputEvent::ScrollDown { x: 77, y: 14 })
        ));
    }

    #[test]
    fn test_sidebar_wheel_scroll_only_applies_inside_sidebar_column() {
        let temp_dir = tempdir().unwrap();
        let cwd = temp_dir.path();
        let mut app = ChatApp::new("Session".to_string(), cwd);

        for idx in 0..120 {
            app.messages.push(tui::ChatMessage::ToolCall {
                name: "edit".to_string(),
                args: "{}".to_string(),
                output: Some(
                    serde_json::json!({
                        "path": format!("src/file-{idx}.rs"),
                        "applied": true,
                        "summary": {"added_lines": 1, "removed_lines": 0},
                        "diff": ""
                    })
                    .to_string(),
                ),
                is_error: Some(false),
            });
        }

        let terminal_rect = Rect {
            x: 0,
            y: 0,
            width: 120,
            height: 40,
        };
        let layout_rects = tui::compute_layout_rects(terminal_rect, &app);
        let sidebar_content = layout_rects
            .sidebar_content
            .expect("sidebar should be visible");
        let main_messages = layout_rects
            .main_messages
            .expect("main messages area should be visible");

        // Test: scrolling in sidebar area scrolls sidebar
        let inside_scrolled = handle_area_scroll(
            &mut app,
            terminal_rect,
            sidebar_content.x,
            sidebar_content.y,
            0,
            3,
        );
        assert!(inside_scrolled);
        assert!(app.sidebar_scroll.offset > 0);

        let previous_sidebar_offset = app.sidebar_scroll.offset;
        let previous_message_offset = app.message_scroll.offset;

        // Test: scrolling in main messages area scrolls messages, not sidebar
        let in_main_scrolled = handle_area_scroll(
            &mut app,
            terminal_rect,
            main_messages.x,
            main_messages.y,
            0,
            3,
        );
        assert!(in_main_scrolled);
        assert!(app.message_scroll.offset > previous_message_offset);
        assert_eq!(app.sidebar_scroll.offset, previous_sidebar_offset);
    }

    #[test]
    fn test_scroll_up_from_auto_scroll_moves_immediately() {
        let temp_dir = tempdir().unwrap();
        let cwd = temp_dir.path();
        let mut app = ChatApp::new("Session".to_string(), cwd);

        for i in 0..120 {
            app.messages
                .push(tui::ChatMessage::Assistant(format!("line {i}")));
        }
        app.mark_dirty();
        app.message_scroll.auto_follow = true;
        app.message_scroll.offset = 0;

        scroll_up_steps(&mut app, 120, 30, 1);

        assert!(!app.message_scroll.auto_follow);
        assert!(app.message_scroll.offset > 0);
    }
}
