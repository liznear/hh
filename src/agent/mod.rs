pub mod color;
pub mod config;
pub mod loader;
pub mod registry;

pub use color::{default_agent_color, parse_color};
pub use config::{AgentConfig, AgentFrontmatter, AgentMode};
pub use loader::AgentLoader;
pub use registry::AgentRegistry;
