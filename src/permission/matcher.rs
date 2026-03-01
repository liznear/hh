use crate::config::Settings;
use crate::core::{ApprovalDecision, ApprovalPolicy};
use crate::permission::policy::Decision;
use crate::tool::ToolSchema;
use std::collections::HashMap;

#[derive(Debug, Clone)]
pub struct PermissionMatcher {
    decisions_by_tool: HashMap<String, Decision>,
}

impl PermissionMatcher {
    pub fn new(settings: Settings, schemas: &[ToolSchema]) -> Self {
        let decisions_by_tool = schemas
            .iter()
            .map(|schema| {
                let capability = schema.capability.as_deref().unwrap_or(schema.name.as_str());
                let raw = capability_policy(&settings, capability);
                (schema.name.clone(), Decision::parse(raw))
            })
            .collect();

        Self { decisions_by_tool }
    }

    pub fn decision_for_tool(&self, tool_name: &str) -> Decision {
        self.decisions_by_tool
            .get(tool_name)
            .copied()
            .unwrap_or(Decision::Deny)
    }
}

impl ApprovalPolicy for PermissionMatcher {
    fn decision_for_tool(&self, tool_name: &str) -> ApprovalDecision {
        match self.decision_for_tool(tool_name) {
            Decision::Allow => ApprovalDecision::Allow,
            Decision::Ask => ApprovalDecision::Ask,
            Decision::Deny => ApprovalDecision::Deny,
        }
    }
}

fn capability_policy<'a>(settings: &'a Settings, capability: &str) -> &'a str {
    let permission = &settings.permission;
    if let Some(raw) = permission.capabilities.get(capability) {
        return raw;
    }

    match capability {
        "read" => &permission.read,
        "list" => &permission.list,
        "glob" => &permission.glob,
        "grep" => &permission.grep,
        "write" => &permission.write,
        "edit" => &permission.edit,
        "todo_write" => &permission.todo_write,
        "todo_read" => &permission.todo_read,
        "question" => &permission.question,
        "bash" => &permission.bash,
        "web" => &permission.web,
        _ => "deny",
    }
}
