use std::{io, time::Duration};

use crossterm::{
    event::{self, Event, KeyCode},
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use hh_widgets::codediff::{CodeDiff, CodeDiffLayout};
use ratatui::{
    Terminal,
    backend::CrosstermBackend,
    widgets::Clear,
};

fn main() -> io::Result<()> {
    let diff = "--- a/src/lib.rs\n+++ b/src/lib.rs\n@@ -1,5 +1,7 @@\n pub fn sum(a: i32, b: i32) -> i32 {\n-    a + b\n+    let total = a + b;\n+    total\n }\n\n pub fn label() -> &'static str {\n-    \"old\"\n+    \"new\"\n }\n";

    let widget = CodeDiff::from_unified_diff(diff).with_layout(CodeDiffLayout::SideBySide);

    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;

    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;
    terminal.clear()?;

    terminal.draw(|f| {
        let area = f.area();
        f.render_widget(Clear, area);
        f.render_widget(widget.clone(), area);
    })?;

    loop {
        if event::poll(Duration::from_millis(100))?
            && let Event::Key(key) = event::read()?
            && matches!(key.code, KeyCode::Char('q') | KeyCode::Esc | KeyCode::Enter)
        {
            break;
        }
    }

    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;
    Ok(())
}
