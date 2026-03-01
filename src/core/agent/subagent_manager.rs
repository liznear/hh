use crate::session::types::{SubAgentFailureReason, SubAgentLifecycleStatus};
use crate::session::{SessionEvent, SessionStore, event_id};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::sync::{Mutex, Semaphore};
use tokio::time::{Duration, sleep};
use uuid::Uuid;

const MAX_EVENT_CONTENT_BYTES: usize = 16 * 1024;
const MAX_PARENT_SUMMARY_BYTES: usize = 2048;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SubagentStatus {
    Pending,
    Running,
    Completed,
    Failed,
    Cancelled,
}

impl SubagentStatus {
    pub fn is_terminal(&self) -> bool {
        matches!(
            self,
            SubagentStatus::Completed | SubagentStatus::Failed | SubagentStatus::Cancelled
        )
    }

    pub fn label(&self) -> &'static str {
        match self {
            SubagentStatus::Pending => "queued",
            SubagentStatus::Running => "running",
            SubagentStatus::Completed => "done",
            SubagentStatus::Failed => "error",
            SubagentStatus::Cancelled => "cancelled",
        }
    }

    fn as_lifecycle_status(&self) -> SubAgentLifecycleStatus {
        match self {
            Self::Pending => SubAgentLifecycleStatus::Pending,
            Self::Running => SubAgentLifecycleStatus::Running,
            Self::Completed => SubAgentLifecycleStatus::Completed,
            Self::Failed => SubAgentLifecycleStatus::Failed,
            Self::Cancelled => SubAgentLifecycleStatus::Cancelled,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubagentNode {
    pub task_id: String,
    pub name: String,
    pub parent_task_id: Option<String>,
    pub parent_session_id: String,
    pub agent_name: String,
    pub prompt: String,
    pub depth: usize,
    pub session_id: String,
    pub status: SubagentStatus,
    pub started_at: u64,
    pub updated_at: u64,
    pub summary: Option<String>,
    pub error: Option<String>,
    pub failure_reason: Option<SubAgentFailureReason>,
    pub progress_seq: u64,
}

#[derive(Debug, Clone)]
pub struct SubagentRequest {
    pub name: String,
    pub description: String,
    pub prompt: String,
    pub subagent_type: String,
    pub resume_task_id: Option<String>,
    pub parent_session_id: String,
    pub parent_task_id: Option<String>,
    pub depth: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct SubagentAcceptance {
    pub task_id: String,
    pub status: String,
    pub message: String,
}

#[derive(Debug, Clone)]
pub struct SubagentExecutionRequest {
    pub task_id: String,
    pub name: String,
    pub parent_session_id: String,
    pub parent_task_id: Option<String>,
    pub description: String,
    pub prompt: String,
    pub subagent_type: String,
    pub child_session_id: String,
    pub depth: usize,
}

#[derive(Debug, Clone)]
pub struct SubagentExecutionResult {
    pub status: SubagentStatus,
    pub summary: String,
    pub error: Option<String>,
    pub failure_reason: Option<SubAgentFailureReason>,
}

type SubagentExecutionFuture = Pin<Box<dyn Future<Output = SubagentExecutionResult> + Send>>;
pub type SubagentExecutor =
    Arc<dyn Fn(SubagentExecutionRequest) -> SubagentExecutionFuture + Send + Sync>;

#[derive(Clone)]
pub struct SubagentManager {
    inner: Arc<Mutex<SubagentManagerState>>,
    queue: Arc<Semaphore>,
    max_depth: usize,
    executor: SubagentExecutor,
}

#[derive(Default)]
struct SubagentManagerState {
    by_task_id: HashMap<String, SubagentNode>,
    children_by_parent: HashMap<String, Vec<String>>,
}

impl SubagentManager {
    pub fn new(max_parallel: usize, max_depth: usize, executor: SubagentExecutor) -> Self {
        Self {
            inner: Arc::new(Mutex::new(SubagentManagerState::default())),
            queue: Arc::new(Semaphore::new(max_parallel.max(1))),
            max_depth,
            executor,
        }
    }

    pub async fn start_or_resume(
        &self,
        request: SubagentRequest,
        parent_session: SessionStore,
    ) -> anyhow::Result<SubagentAcceptance> {
        let child_depth = request.depth.saturating_add(1);
        if child_depth > self.max_depth {
            anyhow::bail!(
                "sub-agent depth {} exceeds configured limit {}",
                child_depth,
                self.max_depth
            );
        }

        let now = now_secs();
        let mut state = self.inner.lock().await;

        let (task_id, child_session_id, should_spawn) =
            if let Some(task_id) = request.resume_task_id.as_ref() {
                let Some(existing) = state.by_task_id.get_mut(task_id) else {
                    anyhow::bail!("unknown task_id '{}'", task_id);
                };
                if existing.parent_session_id != request.parent_session_id {
                    anyhow::bail!(
                        "task_id '{}' is not owned by current parent session",
                        task_id
                    );
                }

                if matches!(
                    existing.status,
                    SubagentStatus::Pending | SubagentStatus::Running
                ) {
                    return Ok(SubagentAcceptance {
                        task_id: task_id.clone(),
                        status: existing.status.label().to_string(),
                        message: "sub-agent is already active".to_string(),
                    });
                }

                existing.status = SubagentStatus::Pending;
                existing.updated_at = now;
                existing.name = request.name.clone();
                existing.summary = None;
                existing.error = None;
                existing.failure_reason = None;

                (task_id.clone(), existing.session_id.clone(), true)
            } else {
                let task_id = Uuid::now_v7().to_string();
                let child_session_id = Uuid::new_v4().to_string();
                let node = SubagentNode {
                    task_id: task_id.clone(),
                    name: request.name.clone(),
                    parent_task_id: request.parent_task_id.clone(),
                    parent_session_id: request.parent_session_id.clone(),
                    agent_name: request.subagent_type.clone(),
                    prompt: request.prompt.clone(),
                    depth: child_depth,
                    session_id: child_session_id.clone(),
                    status: SubagentStatus::Pending,
                    started_at: now,
                    updated_at: now,
                    summary: None,
                    error: None,
                    failure_reason: None,
                    progress_seq: 0,
                };
                state.by_task_id.insert(task_id.clone(), node);
                state
                    .children_by_parent
                    .entry(request.parent_session_id.clone())
                    .or_default()
                    .push(task_id.clone());
                (task_id, child_session_id, true)
            };

        drop(state);

        parent_session.append(&SessionEvent::SubAgentStart {
            id: event_id(),
            task_id: Some(task_id.clone()),
            name: Some(request.name.clone()),
            parent_id: request.parent_task_id.clone(),
            parent_session_id: Some(request.parent_session_id.clone()),
            agent_name: Some(request.subagent_type.clone()),
            session_id: Some(child_session_id.clone()),
            status: SubAgentLifecycleStatus::Pending,
            created_at: now,
            updated_at: now,
            prompt: bounded_text(&request.prompt, MAX_EVENT_CONTENT_BYTES),
            depth: child_depth,
        })?;

        if should_spawn {
            let execution = SubagentExecutionRequest {
                task_id: task_id.clone(),
                name: request.name,
                parent_session_id: request.parent_session_id,
                parent_task_id: request.parent_task_id,
                description: request.description,
                prompt: request.prompt,
                subagent_type: request.subagent_type,
                child_session_id,
                depth: child_depth,
            };
            self.spawn_task(parent_session, execution);
        }

        Ok(SubagentAcceptance {
            task_id,
            status: SubagentStatus::Pending.label().to_string(),
            message: "sub-agent accepted".to_string(),
        })
    }

    pub async fn list_for_parent(&self, parent_session_id: &str) -> Vec<SubagentNode> {
        let state = self.inner.lock().await;
        let mut nodes = state
            .children_by_parent
            .get(parent_session_id)
            .into_iter()
            .flat_map(|task_ids| task_ids.iter())
            .filter_map(|task_id| state.by_task_id.get(task_id))
            .cloned()
            .collect::<Vec<_>>();
        nodes.sort_by(|a, b| {
            a.started_at
                .cmp(&b.started_at)
                .then(a.task_id.cmp(&b.task_id))
        });
        nodes
    }

    pub async fn wait_for_terminal(
        &self,
        parent_session_id: &str,
        task_id: &str,
    ) -> anyhow::Result<SubagentNode> {
        loop {
            let maybe_node = {
                let state = self.inner.lock().await;
                let Some(node) = state.by_task_id.get(task_id) else {
                    anyhow::bail!("unknown task_id '{task_id}'");
                };
                if node.parent_session_id != parent_session_id {
                    anyhow::bail!(
                        "task_id '{}' is not owned by current parent session",
                        task_id
                    );
                }
                if node.status.is_terminal() {
                    Some(node.clone())
                } else {
                    None
                }
            };

            if let Some(node) = maybe_node {
                return Ok(node);
            }

            sleep(Duration::from_millis(50)).await;
        }
    }

    pub async fn wait_for_all(&self, parent_session_id: &str) {
        loop {
            let pending = {
                let state = self.inner.lock().await;
                state
                    .children_by_parent
                    .get(parent_session_id)
                    .into_iter()
                    .flat_map(|task_ids| task_ids.iter())
                    .filter_map(|task_id| state.by_task_id.get(task_id))
                    .any(|node| !node.status.is_terminal())
            };
            if !pending {
                return;
            }
            sleep(Duration::from_millis(50)).await;
        }
    }

    fn spawn_task(&self, parent_session: SessionStore, execution: SubagentExecutionRequest) {
        let queue = Arc::clone(&self.queue);
        let manager = self.clone();
        let executor = Arc::clone(&self.executor);
        tokio::spawn(async move {
            let task_id = execution.task_id.clone();
            let permit = match queue.acquire_owned().await {
                Ok(permit) => permit,
                Err(_) => {
                    manager
                        .finish_task(
                            &parent_session,
                            &task_id,
                            SubagentExecutionResult {
                                status: SubagentStatus::Failed,
                                summary: "sub-agent queue is unavailable".to_string(),
                                error: Some("queue unavailable".to_string()),
                                failure_reason: Some(SubAgentFailureReason::RuntimeError),
                            },
                        )
                        .await;
                    return;
                }
            };

            manager.mark_running(&parent_session, &task_id).await;
            let result = executor(execution).await;
            manager.finish_task(&parent_session, &task_id, result).await;
            drop(permit);
        });
    }

    async fn mark_running(&self, parent_session: &SessionStore, task_id: &str) {
        let mut state = self.inner.lock().await;
        let Some(node) = state.by_task_id.get_mut(task_id) else {
            return;
        };
        if node.status.is_terminal() {
            return;
        }
        node.status = SubagentStatus::Running;
        node.updated_at = now_secs();
        node.progress_seq = node.progress_seq.saturating_add(1);
        let seq = node.progress_seq;
        let _ = parent_session.append(&SessionEvent::SubAgentProgress {
            id: event_id(),
            task_id: Some(task_id.to_string()),
            seq,
            content: "sub-agent execution started".to_string(),
        });
    }

    async fn finish_task(
        &self,
        parent_session: &SessionStore,
        task_id: &str,
        mut result: SubagentExecutionResult,
    ) {
        let mut state = self.inner.lock().await;
        let Some(node) = state.by_task_id.get_mut(task_id) else {
            return;
        };

        if node.status.is_terminal() {
            return;
        }

        if !result.status.is_terminal() {
            result.status = if result.error.is_some() {
                SubagentStatus::Failed
            } else {
                SubagentStatus::Completed
            };
        }

        node.status = result.status.clone();
        node.updated_at = now_secs();
        node.summary = Some(bounded_text(&result.summary, MAX_PARENT_SUMMARY_BYTES));
        node.error = result
            .error
            .as_ref()
            .map(|text| bounded_text(text, MAX_EVENT_CONTENT_BYTES));
        node.failure_reason = result.failure_reason.clone();

        let output = node.error.clone().unwrap_or_else(|| result.summary.clone());
        let _ = parent_session.append(&SessionEvent::SubAgentResult {
            id: event_id(),
            task_id: Some(task_id.to_string()),
            status: node.status.as_lifecycle_status(),
            summary: node.summary.clone(),
            failure_reason: node.failure_reason.clone(),
            is_error: matches!(
                node.status,
                SubagentStatus::Failed | SubagentStatus::Cancelled
            ),
            output: bounded_text(&output, MAX_EVENT_CONTENT_BYTES),
        });
    }
}

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| duration.as_secs())
}

fn bounded_text(input: &str, max_bytes: usize) -> String {
    if input.len() <= max_bytes {
        return input.to_string();
    }

    let mut out = String::with_capacity(max_bytes + 32);
    for ch in input.chars() {
        if out.len() + ch.len_utf8() > max_bytes {
            break;
        }
        out.push(ch);
    }
    out.push_str("\n...[truncated]");
    out
}
