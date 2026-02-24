use crate::core::{Message, Role};
use crate::session::types::{SessionEvent, SessionMetadata};
use anyhow::Context;
use std::fs::{self, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};
use uuid::Uuid;

#[derive(Debug, Clone)]
pub struct SessionStore {
    file: PathBuf,
    metadata_file: PathBuf,
    pub id: String,
}

impl SessionStore {
    /// Creates a new session or loads an existing one.
    /// If `id` is provided, it loads that session.
    /// If `id` is None, it creates a new session.
    /// `title` is used when creating a new session or updating metadata.
    pub fn new(
        root: &Path,
        cwd: &Path,
        id: Option<&str>,
        title: Option<String>,
    ) -> anyhow::Result<Self> {
        let workspace_id = workspace_key(cwd);
        let dir = root.join(&workspace_id);
        fs::create_dir_all(&dir)?;

        // Migration: check if old session file exists at root level (parent of dir)
        // Wait, old path was root.join(format!("{}.jsonl", session_id)); where session_id IS workspace_key.
        // So old file is root/<workspace_id>.jsonl
        let old_file = root.join(format!("{}.jsonl", workspace_id));
        if old_file.exists() {
            // Migrate old session to new format
            let migration_id = Uuid::new_v4().to_string();
            let new_file = dir.join(format!("{}.jsonl", migration_id));
            fs::rename(&old_file, &new_file)?;

            // Create metadata for migrated session
            let meta_file = dir.join(format!("{}.meta.json", migration_id));
            let metadata = SessionMetadata {
                id: migration_id.clone(),
                title: "Migrated Session".to_string(),
                created_at: now(),
                last_updated_at: now(),
            };
            let f = fs::File::create(&meta_file)?;
            serde_json::to_writer(f, &metadata)?;
        }

        let session_id = id
            .map(ToString::to_string)
            .unwrap_or_else(|| Uuid::new_v4().to_string());
        let file = dir.join(format!("{}.jsonl", session_id));
        let metadata_file = dir.join(format!("{}.meta.json", session_id));

        let store = Self {
            file,
            metadata_file,
            id: session_id.clone(),
        };

        // If creating new session (file doesn't exist) and title is provided
        if !store.file.exists() && title.is_some() {
            store.write_metadata(SessionMetadata {
                id: session_id,
                title: title.unwrap(),
                created_at: now(),
                last_updated_at: now(),
            })?;
        }

        Ok(store)
    }

    pub fn list(root: &Path, cwd: &Path) -> anyhow::Result<Vec<SessionMetadata>> {
        let workspace_id = workspace_key(cwd);
        let dir = root.join(&workspace_id);
        if !dir.exists() {
            return Ok(Vec::new());
        }

        let mut sessions = Vec::new();
        for entry in fs::read_dir(dir)? {
            let entry = entry?;
            let path = entry.path();
            if path
                .file_name()
                .map_or(false, |n| n.to_string_lossy().ends_with(".meta.json"))
            {
                // Read metadata
                if let Ok(file) = fs::File::open(&path) {
                    if let Ok(metadata) = serde_json::from_reader::<_, SessionMetadata>(file) {
                        sessions.push(metadata);
                    }
                }
            }
        }
        // Sort by last_updated_at desc
        sessions.sort_by(|a, b| b.last_updated_at.cmp(&a.last_updated_at));
        Ok(sessions)
    }

    fn write_metadata(&self, metadata: SessionMetadata) -> anyhow::Result<()> {
        let file = fs::File::create(&self.metadata_file)?;
        serde_json::to_writer(file, &metadata)?;
        Ok(())
    }

    fn update_timestamp(&self) -> anyhow::Result<()> {
        if self.metadata_file.exists() {
            let file = fs::File::open(&self.metadata_file)?;
            let mut metadata: SessionMetadata = serde_json::from_reader(file)?;
            metadata.last_updated_at = now();
            self.write_metadata(metadata)?;
        }
        Ok(())
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

        // Update timestamp on every append? Or maybe just occasionally?
        // For now, every append ensures accuracy.
        // We ignore errors on metadata update to not break the chat flow if possible,
        // but here we return Result so we should probably handle it.
        // However, if metadata file doesn't exist (e.g. legacy or error), we might skip.
        if self.metadata_file.exists() {
            let _ = self.update_timestamp();
        }

        Ok(())
    }

    pub fn replay_messages(&self) -> anyhow::Result<Vec<Message>> {
        let events = self.replay_events()?;
        let mut messages = Vec::new();

        for event in events {
            match event {
                SessionEvent::Message { message, .. } => messages.push(message),
                SessionEvent::ToolResult { id, output, .. } => {
                    messages.push(Message {
                        role: Role::Tool,
                        content: output,
                        tool_call_id: Some(id),
                    });
                }
                _ => {}
            }
        }
        Ok(messages)
    }

    pub fn replay_events(&self) -> anyhow::Result<Vec<SessionEvent>> {
        if !self.file.exists() {
            return Ok(Vec::new());
        }

        let file = OpenOptions::new().read(true).open(&self.file)?;
        let reader = BufReader::new(file);
        let mut events = Vec::new();

        for line in reader.lines() {
            let line = line?;
            if line.trim().is_empty() {
                continue;
            }
            match serde_json::from_str::<SessionEvent>(&line) {
                Ok(event) => events.push(event),
                Err(e) => {
                    eprintln!("Failed to parse session line: {}", e);
                    continue;
                }
            }
        }

        Ok(events)
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

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs()
}
