use std::path::{Path, PathBuf};
use std::time::Duration;
use std::{fs, io::Cursor};

use base64::Engine;
use crossterm::event::{
    self, Event, KeyCode, KeyEventKind, KeyModifiers, MouseEvent, MouseEventKind,
};
use tokio::sync::mpsc;

use crate::cli::render;
use crate::cli::tui::{self, ChatApp, DebugRenderer, SubmittedInput, TuiEvent, TuiEventSender};
use crate::config::Settings;
use crate::core::agent::{AgentEvents, AgentLoop, NoopEvents};
use crate::core::{Message, MessageAttachment, Role};
use crate::permission::PermissionMatcher;
use crate::provider::openai_compatible::OpenAiCompatibleProvider;
use crate::session::{SessionEvent, SessionStore, event_id};
use crate::tool::registry::ToolRegistry;
use uuid::Uuid;

pub async fn run_chat(settings: Settings, cwd: &std::path::Path) -> anyhow::Result<()> {
    // Setup terminal
    let terminal = tui::setup_terminal()?;
    let mut tui_guard = tui::TuiGuard::new(terminal);

    // Create app state and event channel
    let mut app = ChatApp::new(build_session_name(cwd), cwd, settings.agent.token_budget);
    let (event_tx, mut event_rx) = mpsc::unbounded_channel::<TuiEvent>();
    let event_sender = TuiEventSender::new(event_tx);

    run_interactive_chat_loop(
        &mut tui_guard,
        &mut app,
        InteractiveChatRunner {
            settings: &settings,
            cwd,
            event_sender: &event_sender,
            event_rx: &mut event_rx,
            debug_renderer: None,
            scroll_down_lines: 3,
        },
    )
    .await?;

    Ok(())
}

/// Run interactive chat with debug frame dumping
pub async fn run_chat_with_debug(
    settings: Settings,
    cwd: &std::path::Path,
    debug_dir: PathBuf,
) -> anyhow::Result<()> {
    // Setup terminal
    let terminal = tui::setup_terminal()?;
    let mut tui_guard = tui::TuiGuard::new(terminal);

    // Create debug renderer
    let mut debug_renderer = DebugRenderer::new(debug_dir.clone())?;

    // Create app state and event channel
    let mut app = ChatApp::new(build_session_name(cwd), cwd, settings.agent.token_budget);
    let (event_tx, mut event_rx) = mpsc::unbounded_channel::<TuiEvent>();
    let event_sender = TuiEventSender::new(event_tx);

    run_interactive_chat_loop(
        &mut tui_guard,
        &mut app,
        InteractiveChatRunner {
            settings: &settings,
            cwd,
            event_sender: &event_sender,
            event_rx: &mut event_rx,
            debug_renderer: Some(&mut debug_renderer),
            scroll_down_lines: 1,
        },
    )
    .await?;

    eprintln!(
        "Debug: {} frames written to {}",
        debug_renderer.frame_count(),
        debug_dir.display()
    );

    Ok(())
}

/// Run one prompt in headless debug mode and dump frames to files
pub async fn run_prompt_with_debug(
    settings: Settings,
    cwd: &std::path::Path,
    output_dir: PathBuf,
    prompt: String,
) -> anyhow::Result<()> {
    // Create debug renderer
    let mut renderer = DebugRenderer::new(output_dir.clone())?;

    // Create app state and event channel
    let mut app = ChatApp::new(build_session_name(cwd), cwd, settings.agent.token_budget);
    let (event_tx, mut event_rx) = mpsc::unbounded_channel::<TuiEvent>();
    let event_sender = TuiEventSender::new(event_tx);

    // Submit the prompt
    app.messages.push(tui::ChatMessage::User(prompt.clone()));
    app.set_processing(true);

    // Render initial state with prompt
    renderer.render(&app)?;

    println!(
        "Debug mode: writing screen dumps to {}",
        output_dir.display()
    );

    // Run agent in background
    let settings_clone = settings.clone();
    let cwd_clone = cwd.to_path_buf();
    let sender_clone = event_sender.clone();
    let prompt_clone = prompt.clone();

    let title = prompt.chars().take(50).collect::<String>();
    let title_clone = title.clone();

    let agent_handle = tokio::spawn(async move {
        let result = run_agent(
            settings_clone,
            &cwd_clone,
            Message {
                role: crate::core::Role::User,
                content: prompt_clone,
                attachments: Vec::new(),
                tool_call_id: None,
            },
            sender_clone.clone(),
            None,
            Some(title_clone),
        )
        .await;
        if let Err(ref e) = result {
            sender_clone.send(TuiEvent::Error(e.to_string()));
        }
        result
    });
    drop(event_sender); // Close the channel from this side

    // Main loop - process events and render
    loop {
        tokio::select! {
            event = event_rx.recv() => {
                if let Some(event) = event {
                    let is_done_or_error = matches!(event, TuiEvent::AssistantDone | TuiEvent::Error(_));
                    app.handle_event(&event);

                    // Render after each event
                    renderer.render(&app)?;

                    // Check if processing is done
                    if is_done_or_error {
                        // Render final state
                        renderer.render(&app)?;
                        break;
                    }
                } else {
                    // Channel closed, we're done
                    break;
                }
            }
        }
    }

    if let Err(e) = agent_handle.await? {
        eprintln!("Agent task error: {}", e);
        return Err(e);
    }

    println!(
        "Debug complete: {} frames written to {}",
        renderer.frame_count(),
        output_dir.display()
    );

    Ok(())
}

/// Input event from terminal
enum InputEvent {
    Key(event::KeyEvent),
    Paste(String),
    ScrollUp,
    ScrollDown,
    Refresh,
}

const INPUT_POLL_TIMEOUT: Duration = Duration::from_millis(16);
const INPUT_BATCH_MAX: usize = 64;

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
        KeyCode::Esc => {
            mutate_input(app, ChatApp::clear_input);
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
    if app.auto_scroll {
        app.scroll_offset = total_lines.saturating_sub(visible_height);
        app.auto_scroll = false;
    }

    for _ in 0..steps {
        app.scroll_up();
    }
}

fn scroll_down_steps(app: &mut ChatApp, width: u16, height: u16, steps: usize) {
    if steps == 0 {
        return;
    }

    let (total_lines, visible_height) = scroll_bounds(app, width, height);
    for _ in 0..steps {
        app.scroll_down(total_lines, visible_height);
    }
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
    for _ in 0..visible_height.saturating_sub(1) {
        app.scroll_down(total_lines, visible_height);
    }
}

fn scroll_bounds(app: &ChatApp, width: u16, height: u16) -> (usize, usize) {
    let visible_height = app.message_viewport_height(height);
    let wrap_width = app.message_wrap_width(width);
    let lines = app.get_lines(wrap_width);
    let total_lines = lines.len();
    drop(lines);
    (total_lines, visible_height)
}

fn spawn_agent_task(
    settings: &Settings,
    cwd: &Path,
    input: Message,
    event_sender: &TuiEventSender,
    session_id: Option<String>,
    session_title: Option<String>,
) {
    let settings = settings.clone();
    let cwd = cwd.to_path_buf();
    let sender = event_sender.clone();
    tokio::spawn(async move {
        if let Err(e) = run_agent(
            settings,
            &cwd,
            input,
            sender.clone(),
            session_id,
            session_title,
        )
        .await
        {
            sender.send(TuiEvent::Error(e.to_string()));
        }
    });
}

fn handle_mouse_event(mouse: MouseEvent) -> Option<InputEvent> {
    match mouse.kind {
        MouseEventKind::ScrollUp => Some(InputEvent::ScrollUp),
        MouseEventKind::ScrollDown => Some(InputEvent::ScrollDown),
        _ => None,
    }
}

async fn run_interactive_chat_loop(
    tui_guard: &mut tui::TuiGuard,
    app: &mut ChatApp,
    mut runner: InteractiveChatRunner<'_>,
) -> anyhow::Result<()> {
    if let Some(renderer) = runner.debug_renderer.as_deref_mut() {
        renderer.render(app)?;
    }

    loop {
        tui_guard.get().draw(|f| tui::render_app(f, app))?;
        if let Some(renderer) = runner.debug_renderer.as_deref_mut() {
            renderer.render(app)?;
        }

        tokio::select! {
            input_result = handle_input_batch() => {
                for input_event in input_result? {
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
                    InputEvent::ScrollUp => {
                        let terminal_size = tui_guard.get().size()?;
                        scroll_up_steps(app, terminal_size.width, terminal_size.height, 3);
                    }
                    InputEvent::ScrollDown => {
                        let terminal_size = tui_guard.get().size()?;
                        scroll_down_steps(
                            app,
                            terminal_size.width,
                            terminal_size.height,
                            runner.scroll_down_lines,
                        );
                    }
                    InputEvent::Refresh => {
                        tui_guard.get().autoresize()?;
                        tui_guard.get().clear()?;
                    }
                    }
                }
            }
            event = runner.event_rx.recv() => {
                if let Some(event) = event {
                    app.handle_event(&event);
                }
            }
        }

        if app.should_quit {
            break;
        }
    }

    Ok(())
}

struct InteractiveChatRunner<'a> {
    settings: &'a Settings,
    cwd: &'a Path,
    event_sender: &'a TuiEventSender,
    event_rx: &'a mut mpsc::UnboundedReceiver<TuiEvent>,
    debug_renderer: Option<&'a mut DebugRenderer>,
    scroll_down_lines: usize,
}

fn build_session_name(cwd: &std::path::Path) -> String {
    cwd.file_name()
        .and_then(|name| name.to_str())
        .map(ToString::to_string)
        .unwrap_or_else(|| "Session".to_string())
}

async fn run_agent(
    settings: Settings,
    cwd: &std::path::Path,
    prompt: Message,
    events: impl AgentEvents + 'static,
    session_id: Option<String>,
    session_title: Option<String>,
) -> anyhow::Result<()> {
    let loop_runner = create_agent_loop(settings, cwd, events, session_id, session_title)?;

    loop_runner
        .run(prompt, |_tool_name| {
            // For TUI mode, auto-approve tools (could prompt via TUI in future)
            Ok(true)
        })
        .await?;

    Ok(())
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
    // For single prompt, we create a new session (or we could make it ephemeral if we wanted)
    // Using prompt as title
    let title = prompt.chars().take(50).collect::<String>();
    let loop_runner = create_agent_loop(settings, cwd, events, None, Some(title))?;

    loop_runner
        .run(
            Message {
                role: Role::User,
                content: prompt,
                attachments: Vec::new(),
                tool_call_id: None,
            },
            |tool_name| {
                Ok(render::confirm(&format!(
                    "Allow tool '{}' execution?",
                    tool_name
                ))?)
            },
        )
        .await
}

fn create_agent_loop<E>(
    settings: Settings,
    cwd: &std::path::Path,
    events: E,
    session_id: Option<String>,
    session_title: Option<String>,
) -> anyhow::Result<
    AgentLoop<OpenAiCompatibleProvider, E, ToolRegistry, PermissionMatcher, SessionStore>,
>
where
    E: AgentEvents,
{
    let provider = OpenAiCompatibleProvider::new(
        settings.provider.base_url.clone(),
        settings.provider.model.clone(),
        settings.provider.api_key_env.clone(),
    );

    let tool_registry = ToolRegistry::new(&settings, cwd);
    let tool_schemas = tool_registry.schemas();
    let permissions = PermissionMatcher::new(settings.clone(), &tool_schemas);
    // Use the new session store constructor
    let session = SessionStore::new(
        &settings.session.root,
        cwd,
        session_id.as_deref(),
        session_title,
    )?;

    Ok(AgentLoop {
        provider,
        tools: tool_registry,
        approvals: permissions,
        max_steps: settings.agent.max_steps,
        model: settings.provider.model,
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
    match input.as_str() {
        "/new" => {
            app.start_new_session(build_session_name(cwd));
            finish_idle(app);
        }
        "/compact" => {
            let Some(session_id) = app.session_id.clone() else {
                finish_with_assistant(app, "No active session to compact yet.");
                return;
            };

            app.handle_event(&TuiEvent::CompactionStart);

            if let Ok(handle) = tokio::runtime::Handle::try_current() {
                let settings = settings.clone();
                let cwd = cwd.to_path_buf();
                let sender = event_sender.clone();
                handle.spawn(async move {
                    match compact_session_with_llm(settings, &cwd, &session_id).await {
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
                        rt.block_on(compact_session_with_llm(settings.clone(), cwd, &session_id))
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
) -> anyhow::Result<String> {
    let store = SessionStore::new(&settings.session.root, cwd, Some(session_id), None)
        .context("Failed to load session store")?;
    let messages = store
        .replay_messages()
        .context("Failed to replay session for compaction")?;

    if messages.is_empty() {
        return Ok("No prior context to compact yet.".to_string());
    }

    let summary = generate_compaction_summary(&settings, messages).await?;
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
) -> anyhow::Result<String> {
    #[cfg(test)]
    {
        let _ = settings;
        let _ = messages;
        return Ok("Compacted context summary for tests.".to_string());
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

        let provider = OpenAiCompatibleProvider::new(
            settings.provider.base_url.clone(),
            settings.provider.model.clone(),
            settings.provider.api_key_env.clone(),
        );

        let response = crate::core::Provider::complete(
            &provider,
            crate::core::ProviderRequest {
                model: settings.provider.model.clone(),
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

    let session = &app.available_sessions[idx - 1];
    app.session_id = Some(session.id.clone());
    app.session_name = session.title.clone();
    app.is_picking_session = false;

    let store = SessionStore::new(&settings.session.root, cwd, Some(&session.id), None)
        .context("Failed to load session store")?;

    let events = store.replay_events().context("Failed to replay session")?;

    app.messages.clear();
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
                let rendered_output = result.map(|value| value.output).unwrap_or(output);
                for msg in app.messages.iter_mut().rev() {
                    if let tui::ChatMessage::ToolCall {
                        output: out,
                        is_error: err,
                        ..
                    } = msg
                        && out.is_none()
                    {
                        *out = Some(rendered_output.clone());
                        *err = Some(is_error);
                        break;
                    }
                }
            }
            SessionEvent::Thinking { content, .. } => {
                app.messages.push(tui::ChatMessage::Thinking(content));
            }
            SessionEvent::Compact { summary, .. } => {
                app.messages.push(tui::ChatMessage::Compaction(summary));
            }
            _ => {}
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
        let session_id = app.session_id.clone();
        let session_title = if session_id.is_none() {
            if input.text.is_empty() {
                Some("Image input".to_string())
            } else {
                Some(input.text.chars().take(30).collect::<String>())
            }
        } else {
            None
        };

        let current_session_id = session_id.unwrap_or_else(|| Uuid::new_v4().to_string());
        if app.session_id.is_none() {
            app.session_id = Some(current_session_id.clone());
            if let Some(t) = &session_title {
                app.session_name = t.clone();
            }
        }

        let message = Message {
            role: crate::core::Role::User,
            content: input.text,
            attachments: input.attachments,
            tool_call_id: None,
        };

        spawn_agent_task(
            settings,
            cwd,
            message,
            event_sender,
            Some(current_session_id),
            session_title,
        );
    } else {
        app.set_processing(false);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::settings::{AgentSettings, ProviderSettings, SessionSettings};
    use crate::core::{Message, Role};
    use crossterm::event::KeyEvent;
    use tempfile::tempdir;

    fn create_dummy_settings(root: &Path) -> Settings {
        Settings {
            agent: AgentSettings {
                max_steps: 10,
                token_budget: 1000,
                sub_agent_max_depth: 2,
                system_prompt: None,
            },
            provider: ProviderSettings {
                base_url: "http://localhost:1234".to_string(),
                model: "test-model".to_string(),
                api_key_env: "TEST_KEY".to_string(),
            },
            session: SessionSettings {
                root: root.to_path_buf(),
            },
            tools: Default::default(),
            permission: Default::default(),
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
        let mut app = ChatApp::new("Session".to_string(), cwd, 1000);
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
    fn test_new_starts_fresh_session() {
        let temp_dir = tempdir().unwrap();
        let settings = create_dummy_settings(temp_dir.path());
        let cwd = temp_dir.path();
        let (tx, _rx) = mpsc::unbounded_channel();
        let event_sender = TuiEventSender::new(tx);

        let mut app = ChatApp::new("Session".to_string(), cwd, 1000);
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

        let mut app = ChatApp::new("Session".to_string(), cwd, 1000);
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
    fn test_shift_enter_inserts_newline_without_submitting() {
        let temp_dir = tempdir().unwrap();
        let settings = create_dummy_settings(temp_dir.path());
        let cwd = temp_dir.path();
        let (tx, _rx) = mpsc::unbounded_channel();
        let event_sender = TuiEventSender::new(tx);
        let mut app = ChatApp::new("Session".to_string(), cwd, 1000);
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
        let mut app = ChatApp::new("Session".to_string(), cwd, 1000);
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
        let mut app = ChatApp::new("Session".to_string(), cwd, 1000);
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
        let mut app = ChatApp::new("Session".to_string(), cwd, 1000);

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
        let mut app = ChatApp::new("Session".to_string(), cwd, 1000);
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
        let mut app = ChatApp::new("Session".to_string(), cwd, 1000);
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
        let mut app = ChatApp::new("Session".to_string(), cwd, 1000);
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
        let mut app = ChatApp::new("Session".to_string(), cwd, 1000);
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
        let mut app = ChatApp::new("Session".to_string(), cwd, 1000);
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
    fn test_scroll_up_from_auto_scroll_moves_immediately() {
        let temp_dir = tempdir().unwrap();
        let cwd = temp_dir.path();
        let mut app = ChatApp::new("Session".to_string(), cwd, 1000);

        for i in 0..120 {
            app.messages
                .push(tui::ChatMessage::Assistant(format!("line {i}")));
        }
        app.mark_dirty();
        app.auto_scroll = true;
        app.scroll_offset = 0;

        scroll_up_steps(&mut app, 120, 30, 1);

        assert!(!app.auto_scroll);
        assert!(app.scroll_offset > 0);
    }
}
