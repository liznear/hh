use glob::Pattern;
use serde_json::Value;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub struct RuleContext<'a> {
    pub tool_name: &'a str,
    pub capability: &'a str,
    pub args: &'a Value,
    pub workspace_root: &'a Path,
}

#[derive(Debug, Clone)]
pub struct PermissionRule {
    tool: String,
    specifier: Option<String>,
}

impl PermissionRule {
    pub fn parse_many(raw_rules: &[String]) -> Vec<Self> {
        raw_rules
            .iter()
            .filter_map(|raw| Self::parse(raw))
            .collect()
    }

    pub fn parse(raw: &str) -> Option<Self> {
        let raw = raw.trim();
        if raw.is_empty() {
            return None;
        }

        let (tool, specifier) = if let Some(open) = raw.find('(') {
            if !raw.ends_with(')') {
                return None;
            }
            let tool = raw[..open].trim();
            let inner = raw[open + 1..raw.len() - 1].trim();
            (
                tool,
                (!inner.is_empty()).then(|| normalize_specifier(inner)),
            )
        } else {
            (raw, None)
        };

        if tool.is_empty() {
            return None;
        }

        Some(Self {
            tool: tool.to_string(),
            specifier,
        })
    }

    pub fn matches(&self, context: &RuleContext<'_>) -> bool {
        if let Some(specifier) = &self.specifier {
            return self.matches_with_specifier(specifier, context);
        }

        self.matches_tool(context)
    }

    fn matches_with_specifier(&self, specifier: &str, context: &RuleContext<'_>) -> bool {
        if self.matches_tool_name("bash", context) {
            let Some(command) = context.args.get("command").and_then(Value::as_str) else {
                return false;
            };
            return wildcard_match(specifier, command);
        }

        if self.matches_tool_name("webfetch", context) {
            let Some(expected_domain) = specifier.strip_prefix("domain:") else {
                return false;
            };
            let Some(url) = context.args.get("url").and_then(Value::as_str) else {
                return false;
            };
            return url_domain_matches(url, expected_domain.trim());
        }

        if self.matches_tool_name("read", context)
            || self.matches_tool_name("edit", context)
            || self.matches_tool_name("write", context)
        {
            let Some(candidate_path) = extract_tool_path(context) else {
                return false;
            };

            let Some(resolved_pattern) =
                resolve_permission_path_pattern(specifier, context.workspace_root)
            else {
                return false;
            };

            return glob_path_match(&resolved_pattern, &candidate_path);
        }

        false
    }

    fn matches_tool(&self, context: &RuleContext<'_>) -> bool {
        if self.tool.contains('*') {
            return wildcard_match(&self.tool, context.tool_name);
        }

        self.matches_tool_name(&self.tool, context)
    }

    fn matches_tool_name(&self, tool: &str, context: &RuleContext<'_>) -> bool {
        let normalized = tool.to_ascii_lowercase();
        match normalized.as_str() {
            "bash" => context.capability.eq_ignore_ascii_case("bash"),
            "read" => matches!(
                context.capability,
                "read" | "list" | "glob" | "grep" | "todo_read" | "question" | "skill"
            ),
            "edit" | "write" => matches!(context.capability, "write" | "edit" | "todo_write"),
            "webfetch" => context.tool_name == "web_fetch" || context.capability == "web",
            _ => context.tool_name.eq_ignore_ascii_case(tool),
        }
    }
}

fn normalize_specifier(specifier: &str) -> String {
    if let Some(prefix) = specifier.strip_suffix(":*") {
        return format!("{prefix} *");
    }
    specifier.to_string()
}

fn extract_tool_path(context: &RuleContext<'_>) -> Option<PathBuf> {
    let raw = match context.tool_name {
        "glob" => context.args.get("pattern").and_then(Value::as_str)?,
        "grep" => context
            .args
            .get("path")
            .and_then(Value::as_str)
            .unwrap_or("."),
        _ => context.args.get("path").and_then(Value::as_str)?,
    };

    Some(if Path::new(raw).is_absolute() {
        PathBuf::from(raw)
    } else {
        context.workspace_root.join(raw)
    })
}

fn resolve_permission_path_pattern(specifier: &str, workspace_root: &Path) -> Option<String> {
    let path_pattern = if let Some(stripped) = specifier.strip_prefix("//") {
        stripped.to_string()
    } else if let Some(stripped) = specifier.strip_prefix("~/") {
        let home = dirs::home_dir()?;
        home.join(stripped).display().to_string()
    } else if let Some(stripped) = specifier.strip_prefix('/') {
        workspace_root.join(stripped).display().to_string()
    } else if let Some(stripped) = specifier.strip_prefix("./") {
        workspace_root.join(stripped).display().to_string()
    } else {
        workspace_root.join(specifier).display().to_string()
    };

    Some(normalize_slashes(path_pattern.as_str()))
}

fn glob_path_match(pattern: &str, candidate: &Path) -> bool {
    let candidate = normalize_slashes(candidate.to_string_lossy().as_ref());
    let Ok(glob) = Pattern::new(pattern) else {
        return false;
    };
    glob.matches(candidate.as_str())
}

fn normalize_slashes(input: &str) -> String {
    input.replace('\\', "/")
}

fn url_domain_matches(url: &str, expected_domain: &str) -> bool {
    let Ok(parsed) = reqwest::Url::parse(url) else {
        return false;
    };

    let Some(host) = parsed.host_str() else {
        return false;
    };

    host.eq_ignore_ascii_case(expected_domain)
        || host
            .to_ascii_lowercase()
            .ends_with(format!(".{expected_domain}").as_str())
}

pub fn wildcard_match(pattern: &str, candidate: &str) -> bool {
    let p: Vec<char> = pattern.chars().collect();
    let s: Vec<char> = candidate.chars().collect();

    let mut p_idx = 0usize;
    let mut s_idx = 0usize;
    let mut star_idx: Option<usize> = None;
    let mut backtrack_s = 0usize;

    while s_idx < s.len() {
        if p_idx < p.len() && p[p_idx] == '*' {
            star_idx = Some(p_idx);
            p_idx += 1;
            backtrack_s = s_idx;
        } else if p_idx < p.len() && p[p_idx] == s[s_idx] {
            p_idx += 1;
            s_idx += 1;
        } else if let Some(star) = star_idx {
            p_idx = star + 1;
            backtrack_s += 1;
            s_idx = backtrack_s;
        } else {
            return false;
        }
    }

    while p_idx < p.len() && p[p_idx] == '*' {
        p_idx += 1;
    }

    p_idx == p.len()
}

#[cfg(test)]
mod tests {
    use super::wildcard_match;

    #[test]
    fn wildcard_match_supports_prefix_suffix_and_middle() {
        assert!(wildcard_match("npm run *", "npm run build"));
        assert!(wildcard_match("* --help", "cargo --help"));
        assert!(wildcard_match("git * main", "git checkout main"));
        assert!(!wildcard_match("git push *", "git pull origin main"));
    }
}
