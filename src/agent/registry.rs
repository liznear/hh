use crate::agent::{AgentConfig, AgentMode};
use std::collections::HashMap;

#[derive(Default)]
pub struct AgentRegistry {
    agents: HashMap<String, AgentConfig>,
}

impl AgentRegistry {
    pub fn new(agents: Vec<AgentConfig>) -> Self {
        let mut registry = HashMap::new();
        for agent in agents {
            registry.insert(agent.name.clone(), agent);
        }
        Self { agents: registry }
    }

    pub fn get_agent(&self, name: &str) -> Option<&AgentConfig> {
        self.agents.get(name)
    }

    pub fn list_agents(&self) -> Vec<&AgentConfig> {
        self.agents.values().collect()
    }

    pub fn list_primary_agents(&self) -> Vec<&AgentConfig> {
        self.agents
            .values()
            .filter(|a| a.mode == AgentMode::Primary)
            .collect()
    }

    pub fn register_agent(&mut self, agent: AgentConfig) {
        self.agents.insert(agent.name.clone(), agent);
    }

    pub fn get_primary_agent_names(&self) -> Vec<String> {
        self.agents
            .values()
            .filter(|a| a.mode == AgentMode::Primary)
            .map(|a| a.name.clone())
            .collect()
    }
}
