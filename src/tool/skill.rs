use crate::tool::{Tool, ToolResult, ToolSchema};
use async_trait::async_trait;
use serde::Serialize;
use serde_json::{Value, json};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

pub struct SkillTool {
    skills: BTreeMap<String, SkillEntry>,
}

#[derive(Debug, Clone, Serialize)]
struct SkillEntry {
    name: String,
    description: String,
    path: PathBuf,
}

impl SkillTool {
    pub fn new(workspace_root: PathBuf) -> Self {
        let skills = discover_skills(&workspace_root, dirs::home_dir().as_deref());
        Self { skills }
    }
}

#[async_trait]
impl Tool for SkillTool {
    fn schema(&self) -> ToolSchema {
        ToolSchema {
            name: "skill".to_string(),
            description: format_skill_description(&self.skills),
            capability: Some("read".to_string()),
            mutating: Some(false),
            parameters: json!({
                "type": "object",
                "properties": {
                    "name": {"type": "string"}
                },
                "required": ["name"]
            }),
        }
    }

    async fn execute(&self, args: Value) -> ToolResult {
        let requested_name = args
            .get("name")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty());
        let Some(name) = requested_name else {
            return ToolResult::error("missing required argument: name");
        };

        let Some(entry) = self.skills.get(name) else {
            let available = if self.skills.is_empty() {
                "none".to_string()
            } else {
                self.skills.keys().cloned().collect::<Vec<_>>().join(", ")
            };
            return ToolResult::error(format!("unknown skill '{name}'. available: {available}"));
        };

        let content = match std::fs::read_to_string(&entry.path) {
            Ok(content) => content,
            Err(err) => {
                return ToolResult::error(format!(
                    "failed to read skill at {}: {err}",
                    entry.path.display()
                ));
            }
        };

        ToolResult::ok_text(
            format!("loaded skill {}", entry.name),
            format!(
                "<skill_content name=\"{}\">\n{}\n</skill_content>",
                entry.name, content
            ),
        )
    }
}

fn format_skill_description(skills: &BTreeMap<String, SkillEntry>) -> String {
    let mut description =
        "Load a specialized skill that provides domain-specific instructions and workflows."
            .to_string();

    if skills.is_empty() {
        description.push_str("\n\nNo skills were found in supported skill directories.");
        return description;
    }

    description.push_str("\n\n<available_skills>");
    for skill in skills.values() {
        description.push_str("\n<skill>");
        description.push_str("\n<name>");
        description.push_str(&skill.name);
        description.push_str("</name>");
        description.push_str("\n<description>");
        description.push_str(&skill.description);
        description.push_str("</description>");
        description.push_str("\n<location>");
        description.push_str(&skill.path.display().to_string());
        description.push_str("</location>");
        description.push_str("\n</skill>");
    }
    description.push_str("\n</available_skills>");
    description
}

fn discover_skills(workspace_root: &Path, home_dir: Option<&Path>) -> BTreeMap<String, SkillEntry> {
    let mut skills = BTreeMap::new();
    for root in candidate_skill_roots(workspace_root, home_dir) {
        let discovered = discover_skills_in_root(&root);
        for skill in discovered {
            if !skills.contains_key(&skill.name) {
                skills.insert(skill.name.clone(), skill);
            }
        }
    }
    skills
}

fn candidate_skill_roots(workspace_root: &Path, home_dir: Option<&Path>) -> Vec<PathBuf> {
    let mut roots = vec![
        workspace_root.join(".claude/skills"),
        workspace_root.join(".agents/skills"),
    ];

    if let Some(home) = home_dir {
        roots.push(home.join(".claude/skills"));
        roots.push(home.join(".agents/skills"));
    }

    roots
}

fn discover_skills_in_root(root: &Path) -> Vec<SkillEntry> {
    let Ok(entries) = std::fs::read_dir(root) else {
        return Vec::new();
    };

    let mut entry_paths = entries
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.is_dir())
        .collect::<Vec<_>>();

    entry_paths.sort_by(|left, right| {
        left.file_name()
            .unwrap_or_default()
            .cmp(right.file_name().unwrap_or_default())
    });

    let mut discovered = Vec::new();
    for entry_path in entry_paths {
        let skill_path = entry_path.join("SKILL.md");
        if !skill_path.is_file() {
            continue;
        }

        let content = match std::fs::read_to_string(&skill_path) {
            Ok(content) => content,
            Err(_) => continue,
        };

        let metadata = parse_frontmatter(&content);
        let name = metadata
            .name
            .or_else(|| {
                entry_path
                    .file_name()
                    .and_then(|value| value.to_str())
                    .map(str::to_string)
            })
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty());

        let Some(name) = name else {
            continue;
        };

        let description = metadata
            .description
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty())
            .unwrap_or_else(|| "No description provided".to_string());

        discovered.push(SkillEntry {
            name,
            description,
            path: skill_path,
        });
    }

    discovered
}

#[derive(Default)]
struct Frontmatter {
    name: Option<String>,
    description: Option<String>,
}

fn parse_frontmatter(content: &str) -> Frontmatter {
    let mut lines = content.lines();
    if lines.next() != Some("---") {
        return Frontmatter::default();
    }

    let mut metadata = Frontmatter::default();
    for line in lines {
        if line == "---" {
            break;
        }

        if let Some(value) = line.strip_prefix("name:") {
            metadata.name = Some(trim_yaml_scalar(value));
            continue;
        }

        if let Some(value) = line.strip_prefix("description:") {
            metadata.description = Some(trim_yaml_scalar(value));
        }
    }

    metadata
}

fn trim_yaml_scalar(raw: &str) -> String {
    raw.trim().trim_matches('"').trim_matches('\'').to_string()
}

#[cfg(test)]
mod tests {
    use super::{candidate_skill_roots, discover_skills, parse_frontmatter};
    use std::fs;
    use tempfile::tempdir;

    #[test]
    fn candidate_roots_include_project_then_home() {
        let workspace = tempdir().expect("workspace tempdir");
        let home = tempdir().expect("home tempdir");

        let roots = candidate_skill_roots(workspace.path(), Some(home.path()));
        assert_eq!(roots.len(), 4);
        assert_eq!(roots[0], workspace.path().join(".claude/skills"));
        assert_eq!(roots[1], workspace.path().join(".agents/skills"));
        assert_eq!(roots[2], home.path().join(".claude/skills"));
        assert_eq!(roots[3], home.path().join(".agents/skills"));
    }

    #[test]
    fn project_skill_overrides_home_skill_with_same_name() {
        let workspace = tempdir().expect("workspace tempdir");
        let home = tempdir().expect("home tempdir");

        let project_skill_dir = workspace.path().join(".claude/skills/build-release");
        fs::create_dir_all(&project_skill_dir).expect("create project skill directory");
        fs::write(
            project_skill_dir.join("SKILL.md"),
            "---\nname: build-release\ndescription: project\n---\nproject body",
        )
        .expect("write project skill");

        let home_skill_dir = home.path().join(".agents/skills/build-release");
        fs::create_dir_all(&home_skill_dir).expect("create home skill directory");
        fs::write(
            home_skill_dir.join("SKILL.md"),
            "---\nname: build-release\ndescription: home\n---\nhome body",
        )
        .expect("write home skill");

        let skills = discover_skills(workspace.path(), Some(home.path()));
        let chosen = skills
            .get("build-release")
            .expect("expected discovered skill");

        assert!(chosen.path.starts_with(workspace.path()));
        assert_eq!(chosen.description, "project");
    }

    #[test]
    fn parse_frontmatter_extracts_name_and_description() {
        let metadata = parse_frontmatter(
            "---\nname: test-skill\ndescription: \"does useful work\"\n---\n# Body",
        );

        assert_eq!(metadata.name.as_deref(), Some("test-skill"));
        assert_eq!(metadata.description.as_deref(), Some("does useful work"));
    }
}
