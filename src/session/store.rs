use crate::core::{Message, Role};
use crate::core::{SessionReader, SessionSink};
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
        Self::new_with_parent(root, cwd, id, title, None, None)
    }

    pub fn new_with_parent(
        root: &Path,
        cwd: &Path,
        id: Option<&str>,
        title: Option<String>,
        parent_session_id: Option<String>,
        parent_tool_call_id: Option<String>,
    ) -> anyhow::Result<Self> {
        let workspace_id = workspace_key(cwd);
        let dir = root.join(&workspace_id);
        fs::create_dir_all(&dir)?;

        let old_file = root.join(format!("{}.jsonl", workspace_id));
        if old_file.exists() {
            let migration_id = Uuid::new_v4().to_string();
            let new_file = dir.join(format!("{}.jsonl", migration_id));
            fs::rename(&old_file, &new_file)?;

            let timestamp = now();
            let meta_file = dir.join(format!("{}.meta.json", migration_id));
            let metadata = SessionMetadata {
                id: migration_id.clone(),
                title: "Migrated Session".to_string(),
                created_at: timestamp,
                last_updated_at: timestamp,
                parent_session_id: None,
                is_child_session: false,
                parent_tool_call_id: None,
                runner_state_snapshot: None,
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

        if !store.file.exists()
            && !store.metadata_file.exists()
            && let Some(title) = title
        {
            let timestamp = now();
            store.write_metadata(SessionMetadata {
                id: session_id,
                title,
                created_at: timestamp,
                last_updated_at: timestamp,
                is_child_session: parent_session_id.is_some(),
                parent_session_id,
                parent_tool_call_id,
                runner_state_snapshot: None,
            })?;
        }

        Ok(store)
    }

    pub fn list(root: &Path, cwd: &Path) -> anyhow::Result<Vec<SessionMetadata>> {
        Self::list_with_options(root, cwd, false)
    }

    pub fn list_with_options(
        root: &Path,
        cwd: &Path,
        include_children: bool,
    ) -> anyhow::Result<Vec<SessionMetadata>> {
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
                .is_some_and(|n| n.to_string_lossy().ends_with(".meta.json"))
                && let Ok(file) = fs::File::open(&path)
                && let Ok(metadata) = serde_json::from_reader::<_, SessionMetadata>(file)
            {
                if !include_children && metadata.parent_session_id.is_some() {
                    continue;
                }
                sessions.push(metadata);
            }
        }
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

    pub fn update_title(&self, title: impl Into<String>) -> anyhow::Result<()> {
        let title = title.into();
        let timestamp = now();
        let metadata = if self.metadata_file.exists() {
            let file = fs::File::open(&self.metadata_file)?;
            let mut metadata: SessionMetadata = serde_json::from_reader(file)?;
            metadata.title = title;
            metadata.last_updated_at = timestamp;
            metadata
        } else {
            SessionMetadata {
                id: self.id.clone(),
                title,
                created_at: timestamp,
                last_updated_at: timestamp,
                parent_session_id: None,
                is_child_session: false,
                parent_tool_call_id: None,
                runner_state_snapshot: None,
            }
        };
        self.write_metadata(metadata)
    }

    pub fn load_runner_state_snapshot(
        &self,
    ) -> anyhow::Result<Option<crate::core::agent::RunnerState>> {
        if !self.metadata_file.exists() {
            return Ok(None);
        }

        let file = fs::File::open(&self.metadata_file)?;
        let metadata: SessionMetadata = serde_json::from_reader(file)?;
        Ok(metadata.runner_state_snapshot)
    }

    pub fn save_runner_state_snapshot(
        &self,
        snapshot: &crate::core::agent::RunnerState,
    ) -> anyhow::Result<()> {
        let timestamp = now();
        let mut metadata = if self.metadata_file.exists() {
            let file = fs::File::open(&self.metadata_file)?;
            serde_json::from_reader::<_, SessionMetadata>(file)?
        } else {
            SessionMetadata {
                id: self.id.clone(),
                title: "Session".to_string(),
                created_at: timestamp,
                last_updated_at: timestamp,
                parent_session_id: None,
                is_child_session: false,
                parent_tool_call_id: None,
                runner_state_snapshot: None,
            }
        };

        metadata.last_updated_at = timestamp;
        metadata.runner_state_snapshot = Some(snapshot.clone());
        self.write_metadata(metadata)
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
                SessionEvent::ToolResult {
                    id, output, result, ..
                } => {
                    let content = result.map(|value| value.output).unwrap_or(output);
                    messages.push(Message {
                        tool_calls: Vec::new(),
                        role: Role::Tool,
                        content,
                        attachments: Vec::new(),
                        tool_call_id: Some(id),
                    });
                }
                SessionEvent::Compact { summary, .. } => {
                    messages.clear();
                    messages.push(Message {
                        tool_calls: Vec::new(),
                        role: Role::Assistant,
                        content: summary,
                        attachments: Vec::new(),
                        tool_call_id: None,
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
                }
            }
        }

        Ok(events)
    }

    pub fn file(&self) -> &Path {
        &self.file
    }
}

impl SessionSink for SessionStore {
    fn append(&self, event: &SessionEvent) -> anyhow::Result<()> {
        self.append(event)
    }

    fn save_runner_state_snapshot(
        &self,
        snapshot: &crate::core::agent::RunnerState,
    ) -> anyhow::Result<()> {
        self.save_runner_state_snapshot(snapshot)
    }
}

impl SessionReader for SessionStore {
    fn replay_messages(&self) -> anyhow::Result<Vec<Message>> {
        self.replay_messages()
    }

    fn replay_events(&self) -> anyhow::Result<Vec<SessionEvent>> {
        self.replay_events()
    }

    fn load_runner_state_snapshot(
        &self,
    ) -> anyhow::Result<Option<crate::core::agent::RunnerState>> {
        self.load_runner_state_snapshot()
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
            tool_calls: Vec::new(),
            role: Role::User,
            content,
            attachments: Vec::new(),
            tool_call_id: None,
        },
    }
}

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| duration.as_secs())
}
