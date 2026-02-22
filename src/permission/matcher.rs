use crate::config::Settings;
use crate::permission::policy::Decision;

#[derive(Debug, Clone)]
pub struct PermissionMatcher {
    settings: Settings,
}

impl PermissionMatcher {
    pub fn new(settings: Settings) -> Self {
        Self { settings }
    }

    pub fn decision_for_tool(&self, tool_name: &str) -> Decision {
        let p = &self.settings.permission;
        let raw = match tool_name {
            "read" => &p.read,
            "list" => &p.list,
            "glob" => &p.glob,
            "grep" => &p.grep,
            "write" => &p.write,
            "bash" => &p.bash,
            "web_fetch" => &p.web,
            _ => "deny",
        };
        Decision::parse(raw)
    }
}
