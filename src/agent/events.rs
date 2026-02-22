use serde_json::Value;

pub trait AgentEvents: Send + Sync {
    fn on_thinking(&self, _text: &str) {}
    fn on_tool_start(&self, _name: &str, _args: &Value) {}
    fn on_tool_end(&self, _name: &str, _is_error: bool, _output_preview: &str) {}
    fn on_assistant_delta(&self, _delta: &str) {}
    fn on_assistant_done(&self) {}
}

#[derive(Debug, Default, Clone, Copy)]
pub struct NoopEvents;

impl AgentEvents for NoopEvents {}
