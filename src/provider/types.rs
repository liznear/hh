use crate::core::ToolCall;

#[derive(Debug, Clone, Default)]
pub struct StreamedToolCall {
    pub id: String,
    pub name: String,
    pub arguments_json: String,
}

impl StreamedToolCall {
    pub fn into_tool_call(self) -> ToolCall {
        let arguments = serde_json::from_str(&self.arguments_json)
            .unwrap_or_else(|_| serde_json::Value::Object(Default::default()));
        ToolCall {
            id: self.id,
            name: self.name,
            arguments,
        }
    }
}
