use crate::core::{Message, Role};
use crate::session::types::SessionEvent;
use anyhow::Context;
use std::fs::{self, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use uuid::Uuid;

#[derive(Debug, Clone)]
pub struct SessionStore {
    file: PathBuf,
}

impl SessionStore {
    pub fn for_workspace(root: &Path, cwd: &Path) -> anyhow::Result<Self> {
        fs::create_dir_all(root)?;
        let session_id = workspace_key(cwd);
        let file = root.join(format!("{}.jsonl", session_id));
        Ok(Self { file })
    }

    pub fn append(&self, event: &SessionEvent) -> anyhow::Result<()> {
        if let Some(parent) = self.file.parent() {
            fs::create_dir_all(parent)?;
        }
        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.file)
            .with_context(|| format!("failed opening {}", self.file.display()))?;
        let line = serde_json::to_string(event)?;
        writeln!(file, "{}", line)?;
        Ok(())
    }

    pub fn replay_messages(&self) -> anyhow::Result<Vec<Message>> {
        if !self.file.exists() {
            return Ok(Vec::new());
        }

        let file = OpenOptions::new().read(true).open(&self.file)?;
        let reader = BufReader::new(file);
        let mut messages = Vec::new();

        for line in reader.lines() {
            let line = line?;
            if line.trim().is_empty() {
                continue;
            }
            let event: SessionEvent = serde_json::from_str(&line)?;
            if let SessionEvent::Message { message, .. } = event {
                messages.push(message);
            }
        }

        Ok(messages)
    }

    pub fn file(&self) -> &Path {
        &self.file
    }
}

fn workspace_key(cwd: &Path) -> String {
    let raw = cwd.display().to_string();
    if raw.is_empty() {
        Uuid::new_v4().to_string()
    } else {
        raw.replace('/', "_")
    }
}

pub fn event_id() -> String {
    Uuid::new_v4().to_string()
}

pub fn user_message(content: String) -> SessionEvent {
    SessionEvent::Message {
        id: event_id(),
        message: Message {
            role: Role::User,
            content,
            tool_call_id: None,
        },
    }
}
