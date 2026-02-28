use crate::agent::{AgentConfig, AgentFrontmatter};
use anyhow::Context;
use dirs;
use glob::glob;
use std::path::{Path, PathBuf};

pub struct AgentLoader {
    discovery_paths: Vec<PathBuf>,
}

impl AgentLoader {
    pub fn new() -> anyhow::Result<Self> {
        let cwd = std::env::current_dir()?;

        let discovery_paths = vec![
            // Project-local, highest priority
            cwd.join(".agents/agents"),
            cwd.join(".claude/agents"),
            // User-level
            dirs::home_dir()
                .map(|h| h.join(".agents/agents"))
                .unwrap_or_else(|| PathBuf::from("~/.agents/agents")),
            dirs::home_dir()
                .map(|h| h.join(".claude/agents"))
                .unwrap_or_else(|| PathBuf::from("~/.claude/agents")),
        ];

        Ok(Self { discovery_paths })
    }

    pub fn load_agents(&self) -> anyhow::Result<Vec<AgentConfig>> {
        let mut agents = Vec::new();

        // Start with built-in agents
        agents.push(AgentConfig::builtin_build());
        agents.push(AgentConfig::builtin_plan());

        // Load user-defined agents from discovery paths
        for path in &self.discovery_paths {
            if path.exists() {
                self.load_agents_from_dir(&mut agents, path)?;
            }
        }

        Ok(agents)
    }

    fn load_agents_from_dir(
        &self,
        agents: &mut Vec<AgentConfig>,
        dir: &Path,
    ) -> anyhow::Result<()> {
        let pattern = dir.join("*.md").to_string_lossy().to_string();

        for entry in glob(&pattern).context("Failed to read glob pattern")? {
            let entry = entry.context("Failed to read glob entry")?;
            if let Some(agent) = self.load_agent_from_file(&entry)? {
                // Check if agent with same name already exists
                if let Some(pos) = agents.iter().position(|a| a.name == agent.name) {
                    // Override existing agent
                    agents[pos] = agent;
                } else {
                    agents.push(agent);
                }
            }
        }

        Ok(())
    }

    fn load_agent_from_file(&self, path: &Path) -> anyhow::Result<Option<AgentConfig>> {
        let content = std::fs::read_to_string(path)?;

        // Parse YAML frontmatter
        let (frontmatter, body) = self.parse_frontmatter(&content)?;

        // Use filename (without extension) as agent name
        let name = path
            .file_stem()
            .and_then(|s| s.to_str())
            .context("Invalid agent filename")?
            .to_string();

        Ok(Some(frontmatter.to_agent_config(name, body)))
    }

    fn parse_frontmatter(
        &self,
        content: &str,
    ) -> anyhow::Result<(AgentFrontmatter, Option<String>)> {
        // Check for YAML frontmatter delimited by ---
        if !content.starts_with("---") {
            anyhow::bail!("Agent file must start with YAML frontmatter delimited by ---");
        }

        let rest = &content[3..]; // Skip opening ---
        let Some(frontmatter_end) = rest.find("---") else {
            anyhow::bail!("Missing closing --- for frontmatter");
        };

        let frontmatter_yaml = &rest[..frontmatter_end];
        let body = if frontmatter_end + 3 < rest.len() {
            Some(rest[frontmatter_end + 3..].trim().to_string())
        } else {
            None
        };

        let frontmatter: AgentFrontmatter =
            serde_yaml::from_str(frontmatter_yaml).context("Failed to parse agent frontmatter")?;

        Ok((frontmatter, body))
    }
}

impl Default for AgentLoader {
    fn default() -> Self {
        Self::new().expect("Failed to create AgentLoader")
    }
}
