use crate::config::Settings;
use crate::core::{ApprovalDecision, ApprovalPolicy};
use crate::permission::policy::Decision;
use crate::permission::rules::{PermissionRule, RuleContext};
use crate::tool::ToolSchema;
use serde_json::Value;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub struct PermissionMatcher {
    decisions_by_tool: HashMap<String, Decision>,
    capability_by_tool: HashMap<String, String>,
    allow_rules: Vec<PermissionRule>,
    ask_rules: Vec<PermissionRule>,
    deny_rules: Vec<PermissionRule>,
    workspace_root: PathBuf,
}

impl PermissionMatcher {
    pub fn new(settings: Settings, schemas: &[ToolSchema], workspace_root: &Path) -> Self {
        let mut decisions_by_tool = HashMap::new();
        let mut capability_by_tool = HashMap::new();

        for schema in schemas {
            let capability = schema
                .capability
                .clone()
                .unwrap_or_else(|| schema.name.clone());
            let raw = settings.permission_policy_for_capability(capability.as_str());
            decisions_by_tool.insert(schema.name.clone(), Decision::parse(raw));
            capability_by_tool.insert(schema.name.clone(), capability);
        }

        Self {
            decisions_by_tool,
            capability_by_tool,
            allow_rules: PermissionRule::parse_many(&settings.permissions.allow),
            ask_rules: PermissionRule::parse_many(&settings.permissions.ask),
            deny_rules: PermissionRule::parse_many(&settings.permissions.deny),
            workspace_root: workspace_root.to_path_buf(),
        }
    }

    pub fn decision_for_tool(&self, tool_name: &str) -> Decision {
        self.decisions_by_tool
            .get(tool_name)
            .copied()
            .unwrap_or(Decision::Deny)
    }

    pub fn decision_for_tool_call(&self, tool_name: &str, args: &Value) -> Decision {
        let capability = self
            .capability_by_tool
            .get(tool_name)
            .map(String::as_str)
            .unwrap_or(tool_name);

        let context = RuleContext {
            tool_name,
            capability,
            args,
            workspace_root: &self.workspace_root,
        };

        if self.deny_rules.iter().any(|rule| rule.matches(&context)) {
            return Decision::Deny;
        }
        if self.ask_rules.iter().any(|rule| rule.matches(&context)) {
            return Decision::Ask;
        }
        if self.allow_rules.iter().any(|rule| rule.matches(&context)) {
            return Decision::Allow;
        }

        self.decision_for_tool(tool_name)
    }
}

impl ApprovalPolicy for PermissionMatcher {
    fn decision_for_tool_call(&self, tool_name: &str, args: &Value) -> ApprovalDecision {
        match self.decision_for_tool_call(tool_name, args) {
            Decision::Allow => ApprovalDecision::Allow,
            Decision::Ask => ApprovalDecision::Ask,
            Decision::Deny => ApprovalDecision::Deny,
        }
    }
}
