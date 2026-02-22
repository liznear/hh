use std::fs::{self, File};
use std::io::Write;
use std::path::PathBuf;

use ratatui::{
    backend::TestBackend,
    Terminal,
};

use super::app::ChatApp;
use super::ui;

const DEBUG_WIDTH: u16 = 120;
const DEBUG_HEIGHT: u16 = 40;

pub struct DebugRenderer {
    terminal: Terminal<TestBackend>,
    output_dir: PathBuf,
    frame_count: usize,
    last_buffer: Option<String>,
}

impl DebugRenderer {
    pub fn new(output_dir: PathBuf) -> anyhow::Result<Self> {
        fs::create_dir_all(&output_dir)?;
        let backend = TestBackend::new(DEBUG_WIDTH, DEBUG_HEIGHT);
        let terminal = Terminal::new(backend)?;
        Ok(Self {
            terminal,
            output_dir,
            frame_count: 0,
            last_buffer: None,
        })
    }

    pub fn render(&mut self, app: &ChatApp) -> anyhow::Result<()> {
        self.terminal.draw(|f| {
            ui::render_app(f, app);
        })?;

        // Get current buffer content
        let current = self.buffer_to_string();

        // Only dump if content changed
        if self.last_buffer.as_ref() != Some(&current) {
            self.dump_screen(&current)?;
            self.frame_count += 1;
            self.last_buffer = Some(current);
        }

        Ok(())
    }

    fn buffer_to_string(&self) -> String {
        let buffer = self.terminal.backend().buffer();
        let mut result = String::new();

        for y in 0..DEBUG_HEIGHT {
            let mut line = String::new();
            for x in 0..DEBUG_WIDTH {
                let cell = &buffer[(x, y)];
                line.push_str(cell.symbol());
            }
            result.push_str(line.trim_end());
            result.push('\n');
        }

        result
    }

    fn dump_screen(&self, content: &str) -> anyhow::Result<()> {
        let filename = format!("screen-{:03}.txt", self.frame_count);
        let path = self.output_dir.join(filename);

        let mut file = File::create(&path)?;
        write!(file, "{}", content)?;

        Ok(())
    }

    pub fn output_dir(&self) -> &std::path::Path {
        &self.output_dir
    }

    pub fn frame_count(&self) -> usize {
        self.frame_count
    }
}
