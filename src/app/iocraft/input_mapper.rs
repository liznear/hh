use crate::app::events::InputEvent;
use crossterm::event as ct;
use iocraft::prelude as io;

pub fn map_key_event(event: &io::KeyEvent) -> ct::KeyEvent {
    let code = match event.code {
        io::KeyCode::Char(c) => ct::KeyCode::Char(c),
        io::KeyCode::Enter => ct::KeyCode::Enter,
        io::KeyCode::Left => ct::KeyCode::Left,
        io::KeyCode::Right => ct::KeyCode::Right,
        io::KeyCode::Up => ct::KeyCode::Up,
        io::KeyCode::Down => ct::KeyCode::Down,
        io::KeyCode::Home => ct::KeyCode::Home,
        io::KeyCode::End => ct::KeyCode::End,
        io::KeyCode::PageUp => ct::KeyCode::PageUp,
        io::KeyCode::PageDown => ct::KeyCode::PageDown,
        io::KeyCode::Tab => ct::KeyCode::Tab,
        io::KeyCode::BackTab => ct::KeyCode::BackTab,
        io::KeyCode::Delete => ct::KeyCode::Delete,
        io::KeyCode::Insert => ct::KeyCode::Insert,
        io::KeyCode::F(n) => ct::KeyCode::F(n),
        io::KeyCode::Esc => ct::KeyCode::Esc,
        io::KeyCode::Backspace => ct::KeyCode::Backspace,
        io::KeyCode::Null => ct::KeyCode::Null,
        _ => ct::KeyCode::Null,
    };

    let mut modifiers = ct::KeyModifiers::empty();
    if event.modifiers.contains(io::KeyModifiers::SHIFT) {
        modifiers.insert(ct::KeyModifiers::SHIFT);
    }
    if event.modifiers.contains(io::KeyModifiers::CONTROL) {
        modifiers.insert(ct::KeyModifiers::CONTROL);
    }
    if event.modifiers.contains(io::KeyModifiers::ALT) {
        modifiers.insert(ct::KeyModifiers::ALT);
    }

    let kind = match event.kind {
        io::KeyEventKind::Press => ct::KeyEventKind::Press,
        io::KeyEventKind::Release => ct::KeyEventKind::Release,
        io::KeyEventKind::Repeat => ct::KeyEventKind::Repeat,
    };

    ct::KeyEvent {
        code,
        modifiers,
        kind,
        state: ct::KeyEventState::empty(),
    }
}

pub fn map_terminal_event(event: &io::TerminalEvent) -> Option<InputEvent> {
    match event {
        io::TerminalEvent::Key(k) => Some(InputEvent::Key(map_key_event(k))),
        io::TerminalEvent::FullscreenMouse(m) => match m.kind {
            io::MouseEventKind::Down(ct::MouseButton::Left) => Some(InputEvent::MouseClick {
                x: m.column,
                y: m.row,
            }),
            io::MouseEventKind::Drag(ct::MouseButton::Left) => Some(InputEvent::MouseDrag {
                x: m.column,
                y: m.row,
            }),
            io::MouseEventKind::Up(ct::MouseButton::Left) => Some(InputEvent::MouseRelease {
                x: m.column,
                y: m.row,
            }),
            io::MouseEventKind::ScrollUp => Some(InputEvent::ScrollUp {
                x: m.column,
                y: m.row,
            }),
            io::MouseEventKind::ScrollDown => Some(InputEvent::ScrollDown {
                x: m.column,
                y: m.row,
            }),
            _ => None,
        },
        _ => None,
    }
}
