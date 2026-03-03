# MCP Server Support Implementation Plan

## Overview

Add Model Context Protocol (MCP) server support to allow external tools to be integrated into the agent runtime, matching the configuration schema used by OpenCode and Claude Desktop.

---

## 1. Configuration Schema (Matching OpenCode/Claude)

**Location**: Extend `src/config/settings.rs`

### Schema Structure

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpSettings {
    #[serde(default)]
    pub servers: BTreeMap<String, McpServerConfig>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum McpServerConfig {
    #[serde(rename = "local")]
    Stdio {
        command: Vec<String>,
        #[serde(default)]
        environment: BTreeMap<String, String>,
        #[serde(default = "default_mcp_timeout")]
        timeout: u64,
        #[serde(default = "default_true")]
        enabled: bool,
    },
    #[serde(rename = "remote")]
    Http {
        url: String,
        #[serde(default)]
        headers: BTreeMap<String, String>,
        #[serde(default)]
        oauth: Option<McpOAuthConfig>,
        #[serde(default = "default_mcp_timeout")]
        timeout: u64,
        #[serde(default = "default_true")]
        enabled: bool,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpOAuthConfig {
    #[serde(default)]
    client_id: Option<String>,
    #[serde(default)]
    client_secret: Option<String>,
    #[serde(default)]
    scope: Option<String>,
}

fn default_mcp_timeout() -> u64 { 5000 }
fn default_true() -> bool { true }
```

### Example Configuration

In `hh.json`:

```json
{
  "mcp": {
    "servers": {
      "filesystem": {
        "type": "local",
        "command": ["npx", "-y", "@modelcontextprotocol/server-filesystem", "/path/to/project"],
        "enabled": true
      },
      "github": {
        "type": "local",
        "command": ["npx", "-y", "@modelcontextprotocol/server-github"],
        "environment": {
          "GITHUB_TOKEN": "{env:GITHUB_TOKEN}"
        }
      },
      "context7": {
        "type": "remote",
        "url": "https://mcp.context7.com/mcp",
        "headers": {
          "CONTEXT7_API_KEY": "{env:CONTEXT7_API_KEY}"
        }
      }
    }
  }
}
```

### Tasks

- Add `McpSettings` to `Settings` struct
- Update config loader to support variable substitution `{env:VAR}` and `{file:PATH}`
- Add precedence merging for remote/project/user configs
- Add `mcp: McpSettings` field to `Settings` with `#[serde(default)]`

---

## 2. MCP Client Library Integration

### Recommended Library

**`rmcp`** (official Rust MCP SDK) or **`rust-mcp-sdk`**

### Add to `Cargo.toml`

```toml
rmcp = { version = "0.1", features = ["client", "transport-io", "transport-sse"] }
```

### Why rmcp

- Official MCP Rust SDK
- Supports stdio and HTTP/SSE transports
- Type-safe protocol implementation
- Async/await with tokio
- Active maintenance

### Alternative

`rust-mcp-sdk` also supports streamable HTTP transport and has more features.

---

## 3. MCP Server Manager & Tool Registry Integration

### New Module Structure

Create `src/mcp/mod.rs` with submodules:

```
src/mcp/
├── mod.rs           # Public API and manager
├── client.rs        # MCP client wrapper
├── tool.rs          # MCP tool implementation
└── transport.rs     # Transport abstractions
```

### Components

#### a. MCP Client Manager

```rust
// src/mcp/mod.rs

pub struct McpClientManager {
    clients: HashMap<String, Arc<McpClient>>,
}

impl McpClientManager {
    pub async fn from_config(config: &McpSettings) -> Result<Self> {
        // Initialize all enabled MCP servers
        // Discover tools from each server
        // Handle initialization failures gracefully
    }
    
    pub fn tools(&self) -> Vec<Arc<dyn Tool>> {
        // Collect all MCP tools from all clients
    }
    
    pub async fn shutdown(&self) {
        // Gracefully close all connections
    }
}
```

#### b. MCP Client Wrapper

```rust
// src/mcp/client.rs

pub struct McpClient {
    name: String,
    client: rmcp::Client,
    tools: Vec<McpToolInfo>,
}

pub struct McpToolInfo {
    name: String,
    description: String,
    input_schema: Value,
}

impl McpClient {
    pub async fn connect_stdio(config: &StdioConfig) -> Result<Self> {
        // Create stdio transport
        // Initialize MCP protocol
        // Discover available tools
    }
    
    pub async fn connect_http(config: &HttpConfig) -> Result<Self> {
        // Create HTTP/SSE transport
        // Initialize MCP protocol
        // Discover available tools
    }
    
    pub async fn call_tool(&self, name: &str, args: Value) -> Result<ToolResult> {
        // Execute tool on MCP server
        // Convert MCP response to ToolResult
    }
}
```

#### c. MCP Tool Wrapper

```rust
// src/mcp/tool.rs

pub struct McpTool {
    server_name: String,
    tool_name: String,
    full_name: String, // "{server_name}_{tool_name}"
    schema: ToolSchema,
    client: Arc<McpClient>,
}

#[async_trait]
impl Tool for McpTool {
    fn schema(&self) -> ToolSchema {
        self.schema.clone()
    }
    
    async fn execute(&self, args: Value) -> ToolResult {
        self.client.call_tool(&self.tool_name, args).await
    }
}
```

### Integration Point

**File**: `src/tool/registry.rs`

```rust
impl ToolRegistry {
    pub fn new_with_mcp(
        settings: &Settings,
        workspace_root: &Path,
        mcp_manager: &McpClientManager,
    ) -> Self {
        let mut registry = Self::new(settings, workspace_root);
        
        // Register MCP tools
        for tool in mcp_manager.tools() {
            registry.register_mcp_tool(tool);
        }
        
        registry
    }
    
    fn register_mcp_tool(&mut self, tool: Arc<dyn Tool>) {
        let name = tool.schema().name.clone();
        self.tools.insert(name, tool);
    }
}
```

### Tool Naming Convention

- Prefix all MCP tools with server name to avoid collisions
- Example: `github_search_issues`, `github_list_repos`
- Format: `{server_name}_{tool_name}`

---

## 4. Agent State Integration

### Changes to `src/core/agent/state.rs`

- MCP servers are stateless from agent perspective
- Tools discovered from MCP are included in `ToolExecutor::schemas()`
- No additional state needed in `AgentState`

### No changes required to `AgentState` struct

MCP tools integrate seamlessly through the existing `ToolExecutor` trait.

---

## 5. System Prompt Injection

### Approach 1: Automatic via Tool Schemas

MCP tools are automatically available through the tool schema mechanism - no changes needed.

### Approach 2: Optional Metadata Injection

Add MCP server metadata to system prompt for better LLM understanding.

#### Update System Prompt Templates

**File**: `src/core/prompts/build_system_prompt.md` (and others)

Add at the end:

```markdown
{% if mcp_servers %}
## Available MCP Servers

The following MCP (Model Context Protocol) servers provide additional tools:

{% for server in mcp_servers %}
- **{{ server.name }}**: {{ server.description | default(value="") }}
{% endfor %}

You can use tools from these servers by their prefixed names (e.g., `github_search_issues`).
{% endif %}
```

#### Implementation

**Option A**: Create a simple template system using string replacement

**Option B**: Dynamically append MCP metadata to system prompt in `AgentLoop`

```rust
impl AgentLoop {
    fn build_system_prompt(&self, mcp_manager: &McpClientManager) -> String {
        let mut prompt = self.system_prompt.clone();
        
        if !mcp_manager.clients.is_empty() {
            prompt.push_str("\n\n## Available MCP Servers\n\n");
            for (name, client) in &mcp_manager.clients {
                prompt.push_str(&format!("- **{}**: {} tools available\n", 
                    name, client.tools.len()));
            }
        }
        
        prompt
    }
}
```

**Recommendation**: Start with Option B (simple string append), add templating later if needed.

---

## 6. Permission System Integration

### Extend Permission Settings

**File**: `src/config/settings.rs`

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PermissionSettings {
    // ... existing fields
    
    #[serde(default)]
    pub mcp: McpPermissionSettings,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct McpPermissionSettings {
    #[serde(default)]
    pub servers: BTreeMap<String, String>,  // server_name -> policy
    #[serde(default)]
    pub tools: BTreeMap<String, String>,    // tool_pattern -> policy
}
```

### Example Configuration

```json
{
  "permissions": {
    "mcp": {
      "servers": {
        "github": "ask",
        "filesystem": "allow"
      },
      "tools": {
        "github_*": "ask",
        "filesystem_read*": "allow",
        "filesystem_write*": "ask"
      }
    }
  }
}
```

### Permission Matching Logic

**File**: `src/permission/matcher.rs`

```rust
impl PermissionMatcher {
    pub fn match_mcp_tool(&self, server: &str, tool: &str) -> Decision {
        let full_name = format!("{}_{}", server, tool);
        
        // Check tool-specific patterns first
        for (pattern, policy) in &self.settings.mcp.tools {
            if self.matches_pattern(&full_name, pattern) {
                return policy_to_decision(policy);
            }
        }
        
        // Check server-level policies
        if let Some(policy) = self.settings.mcp.servers.get(server) {
            return policy_to_decision(policy);
        }
        
        // Default: ask for permission
        Decision::Ask
    }
}
```

### Tasks

- Extend `PermissionSettings` with MCP fields
- Update permission matcher to support MCP tool patterns
- Integrate with existing approval workflow in `AgentLoop`
- Apply permissions to MCP tool capability field

---

## 7. Session Persistence

### No Changes Required

**File**: `src/session/types.rs`

MCP tool calls/responses already fit the existing `SessionEvent` model:

- Use existing `ToolCall` event (name will be prefixed: `github_search_issues`)
- Use existing `ToolResult` event
- MCP server state is external, not persisted in session
- Replay works automatically

### Session Events Flow

```
User Prompt → Tool Call (github_search) → MCP Server → Tool Result → Session Event
```

---

## 8. Transport Implementation

### a. Stdio Transport (Local MCP Servers)

**File**: `src/mcp/transport.rs`

```rust
use rmcp::transport::TokioChildProcess;
use tokio::process::Command;

pub async fn connect_stdio(config: &McpServerConfig::Stdio) -> Result<McpClient> {
    let mut cmd = Command::new(&config.command[0]);
    cmd.args(&config.command[1..]);
    
    // Set environment variables
    for (key, value) in &config.environment {
        let resolved = resolve_env_var(value)?; // Handle {env:VAR} substitution
        cmd.env(key, resolved);
    }
    
    // Create transport
    let transport = TokioChildProcess::new(cmd)
        .map_err(|e| anyhow::anyhow!("Failed to create stdio transport: {}", e))?;
    
    // Initialize client
    let client = rmcp::Client::new(transport);
    client.initialize().await
        .map_err(|e| anyhow::anyhow!("Failed to initialize MCP client: {}", e))?;
    
    // Discover tools
    let tools = client.list_tools().await?;
    
    Ok(McpClient { /* ... */ })
}

fn resolve_env_var(value: &str) -> Result<String> {
    if value.starts_with("{env:") && value.ends_with("}") {
        let var_name = &value[5..value.len()-1];
        std::env::var(var_name)
            .map_err(|_| anyhow::anyhow!("Environment variable {} not set", var_name))
    } else {
        Ok(value.to_string())
    }
}
```

### b. HTTP/SSE Transport (Remote MCP Servers)

```rust
use rmcp::transport::SseTransport;

pub async fn connect_http(config: &McpServerConfig::Http) -> Result<McpClient> {
    // Resolve header values (handle {env:VAR} substitution)
    let mut headers = HashMap::new();
    for (key, value) in &config.headers {
        headers.insert(key.clone(), resolve_env_var(value)?);
    }
    
    // Create transport
    let transport = SseTransport::new(&config.url, headers);
    
    // Initialize client
    let client = rmcp::Client::new(transport);
    client.initialize().await?;
    
    // Discover tools
    let tools = client.list_tools().await?;
    
    Ok(McpClient { /* ... */ })
}
```

### c. OAuth Support (for Remote Servers)

**OAuth Flow**:

1. Detect 401 response from MCP server
2. Initiate OAuth 2.0 authorization flow
3. Support Dynamic Client Registration (RFC 7591) if available
4. Store tokens securely

**Token Storage**: `~/.local/share/hh/mcp-auth.json`

```rust
// src/mcp/oauth.rs

pub struct OAuthManager {
    token_store: PathBuf,
}

impl OAuthManager {
    pub async fn authenticate(&self, server: &str, config: &McpOAuthConfig) -> Result<()> {
        // 1. Discover OAuth endpoints from server
        // 2. Open browser for authorization
        // 3. Handle callback
        // 4. Exchange code for tokens
        // 5. Store tokens
    }
    
    pub fn get_token(&self, server: &str) -> Result<Option<Token>> {
        // Load token from storage
    }
    
    pub fn logout(&self, server: &str) -> Result<()> {
        // Remove token from storage
    }
}
```

---

## 9. CLI Commands

### New Commands

**File**: `src/cli/commands.rs`

```rust
#[derive(Subcommand)]
pub enum Commands {
    // ... existing commands
    
    /// Manage MCP servers
    Mcp {
        #[command(subcommand)]
        command: McpCommand,
    },
}

#[derive(Subcommand)]
pub enum McpCommand {
    /// List all configured MCP servers and their status
    List,
    
    /// Authenticate with an OAuth-enabled MCP server
    Auth {
        /// Name of the MCP server
        server_name: String,
    },
    
    /// Debug MCP server connection
    Debug {
        /// Name of the MCP server
        server_name: String,
    },
    
    /// Logout from an MCP server (remove stored credentials)
    Logout {
        /// Name of the MCP server
        server_name: String,
    },
}
```

### Usage Examples

```bash
# List all configured MCP servers
hh mcp list

# Authenticate with OAuth-enabled server
hh mcp auth sentry

# Debug connection issues
hh mcp debug context7

# Remove stored credentials
hh mcp logout github
```

### Output Format for `hh mcp list`

```
MCP Servers:
  ✓ filesystem (local)
    Tools: read_file, write_file, list_directory
    
  ✓ github (local)
    Tools: search_issues, list_repos, create_issue
    
  ✗ context7 (remote) - Authentication required
    Run: hh mcp auth context7
    
  ✗ sentry (remote) - Connection failed: timeout
```

---

## 10. Error Handling & Recovery

### Scenarios

1. **MCP server fails to start (stdio)**
   - Log error with command and environment
   - Mark server as unavailable
   - Continue with other servers

2. **MCP server becomes unavailable (HTTP)**
   - Return error on tool execution
   - Optionally attempt reconnection

3. **Tool execution timeout**
   - Enforce timeout from configuration (default 5000ms)
   - Return timeout error to LLM

4. **Invalid tool schema**
   - Log warning and skip tool
   - Continue with valid tools

### Graceful Degradation Strategy

```rust
impl McpClientManager {
    pub async fn from_config(config: &McpSettings) -> Self {
        let mut clients = HashMap::new();
        
        for (name, server_config) in &config.servers {
            if !server_config.enabled() {
                continue;
            }
            
            match McpClient::connect(server_config).await {
                Ok(client) => {
                    clients.insert(name.clone(), Arc::new(client));
                }
                Err(e) => {
                    eprintln!("Warning: Failed to connect to MCP server '{}': {}", name, e);
                    // Continue with other servers
                }
            }
        }
        
        Self { clients }
    }
}
```

### Retry Logic (HTTP Transport)

```rust
pub async fn call_tool_with_retry(
    &self,
    name: &str,
    args: Value,
    max_retries: u32,
) -> Result<ToolResult> {
    let mut retries = 0;
    
    loop {
        match self.client.call_tool(name, args.clone()).await {
            Ok(result) => return Ok(result),
            Err(e) if retries < max_retries && is_retryable(&e) => {
                retries += 1;
                tokio::time::sleep(Duration::from_millis(100 * 2u64.pow(retries))).await;
            }
            Err(e) => return Err(e),
        }
    }
}
```

---

## 11. Implementation Order

### Phase 1: Foundation (Priority: High) - 2-3 days

1. **Add MCP configuration schema to `Settings`**
   - Define `McpSettings`, `McpServerConfig`, `McpOAuthConfig`
   - Add to `Settings` struct
   - Update serialization/deserialization

2. **Add `rmcp` dependency**
   - Add to `Cargo.toml`
   - Verify build

3. **Create `McpClientManager` for connection management**
   - Create `src/mcp/mod.rs`
   - Implement basic structure
   - Add initialization logic

4. **Implement stdio transport support**
   - Create transport wrapper
   - Handle environment variable substitution
   - Test with local MCP server

### Phase 2: Tool Integration (Priority: High) - 2-3 days

5. **Create `McpTool` wrapper implementing `Tool` trait**
   - Implement `schema()` method
   - Implement `execute()` method
   - Handle result conversion

6. **Integrate MCP tools into `ToolRegistry`**
   - Add `new_with_mcp()` constructor
   - Register MCP tools with prefixes
   - Test tool discovery

7. **Test tool discovery and execution**
   - Use `@modelcontextprotocol/server-everything` for testing
   - Verify tool schemas are correct
   - Verify tool execution works

### Phase 3: Permissions & UX (Priority: Medium) - 1-2 days

8. **Add MCP permission settings**
   - Extend `PermissionSettings`
   - Add permission matching logic

9. **Integrate with approval workflow**
   - Apply permissions to MCP tool calls
   - Test permission enforcement

10. **Add CLI commands for MCP management**
    - Implement `mcp list`
    - Implement `mcp auth`
    - Implement `mcp debug`
    - Implement `mcp logout`

### Phase 4: Advanced Features (Priority: Low) - 2-3 days

11. **Implement HTTP/SSE transport**
    - Add HTTP transport support
    - Test with remote MCP server

12. **Add OAuth support**
    - Implement OAuth flow
    - Add token storage
    - Test authentication

13. **Add reconnection logic**
    - Implement retry on failure
    - Add connection health checks

14. **Add system prompt metadata injection**
    - Build dynamic system prompt
    - Test with multiple MCP servers

---

## 12. Testing Strategy

### Unit Tests

**File**: `src/mcp/tests.rs` (or inline)

```rust
#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_mcp_config_parsing() {
        let json = r#"{
            "servers": {
                "test": {
                    "type": "local",
                    "command": ["npx", "test-server"],
                    "enabled": true
                }
            }
        }"#;
        
        let config: McpSettings = serde_json::from_str(json).unwrap();
        assert_eq!(config.servers.len(), 1);
    }
    
    #[test]
    fn test_tool_name_prefixing() {
        let tool = McpTool::new("github", "search_issues", /* ... */);
        assert_eq!(tool.schema().name, "github_search_issues");
    }
    
    #[test]
    fn test_permission_matching() {
        let matcher = PermissionMatcher::new(/* ... */);
        let decision = matcher.match_mcp_tool("github", "search_issues");
        assert_eq!(decision, Decision::Ask);
    }
}
```

### Integration Tests

**File**: `tests/mcp_integration.rs`

```rust
#[tokio::test]
#[ignore] // Requires MCP server to be available
async fn test_stdio_transport() {
    let config = McpSettings {
        servers: [(
            "everything".to_string(),
            McpServerConfig::Stdio {
                command: vec!["npx".to_string(), "-y".to_string(), 
                              "@modelcontextprotocol/server-everything".to_string()],
                environment: BTreeMap::new(),
                timeout: 5000,
                enabled: true,
            },
        )].into_iter().collect(),
    };
    
    let manager = McpClientManager::from_config(&config).await.unwrap();
    assert!(manager.clients.contains_key("everything"));
    
    let tools = manager.tools();
    assert!(!tools.is_empty());
}

#[tokio::test]
#[ignore]
async fn test_tool_execution() {
    // Initialize MCP client
    // Call a tool
    // Verify result
}
```

### Manual Testing Checklist

- [ ] Configure multiple MCP servers in `hh.json`
- [ ] Start `hh chat` and verify servers connect
- [ ] Use an MCP tool in a prompt
- [ ] Verify tool execution works
- [ ] Test with GitHub MCP server
- [ ] Test with filesystem MCP server
- [ ] Test permission enforcement (ask/allow/deny)
- [ ] Test OAuth flow with remote server
- [ ] Test `hh mcp list` command
- [ ] Test `hh mcp auth` command
- [ ] Test error handling when server fails
- [ ] Test timeout enforcement

---

## 13. Documentation

### Update AGENTS.md

Add new section:

```markdown
## MCP Servers

The agent supports Model Context Protocol (MCP) servers for extending tool capabilities.

### Configuration

Add MCP servers to `hh.json`:

\`\`\`json
{
  "mcp": {
    "servers": {
      "github": {
        "type": "local",
        "command": ["npx", "-y", "@modelcontextprotocol/server-github"],
        "environment": {
          "GITHUB_TOKEN": "{env:GITHUB_TOKEN}"
        },
        "enabled": true
      },
      "context7": {
        "type": "remote",
        "url": "https://mcp.context7.com/mcp",
        "headers": {
          "CONTEXT7_API_KEY": "{env:CONTEXT7_API_KEY}"
        }
      }
    }
  }
}
\`\`\`

### Configuration Options

**Local (stdio) servers**:
- `type`: Must be `"local"`
- `command`: Array of command and arguments to start the server
- `environment`: Environment variables (supports `{env:VAR}` substitution)
- `timeout`: Timeout in milliseconds (default: 5000)
- `enabled`: Enable/disable server (default: true)

**Remote (HTTP) servers**:
- `type`: Must be `"remote"`
- `url`: URL of the MCP server
- `headers`: HTTP headers (supports `{env:VAR}` substitution)
- `timeout`: Timeout in milliseconds (default: 5000)
- `enabled`: Enable/disable server (default: true)
- `oauth`: OAuth configuration (optional)

### CLI Commands

- `hh mcp list` - List configured MCP servers and their status
- `hh mcp auth <server>` - Authenticate with OAuth-enabled server
- `hh mcp debug <server>` - Debug connection issues
- `hh mcp logout <server>` - Remove stored credentials

### Permissions

Control MCP tool access in the permissions configuration:

\`\`\`json
{
  "permissions": {
    "mcp": {
      "servers": {
        "github": "ask"
      },
      "tools": {
        "github_*": "ask",
        "filesystem_read*": "allow"
      }
    }
  }
}
\`\`\`

### Tool Naming

MCP tools are prefixed with the server name to avoid collisions:
- Format: `{server_name}_{tool_name}`
- Example: `github_search_issues`, `filesystem_read_file`

### Example MCP Servers

- **Filesystem**: `@modelcontextprotocol/server-filesystem`
- **GitHub**: `@modelcontextprotocol/server-github`
- **PostgreSQL**: `@modelcontextprotocol/server-postgres`
- **Context7**: `https://mcp.context7.com/mcp`
- **Sentry**: `https://mcp.sentry.dev/mcp`
```

### Create MCP Guide (Optional)

**File**: `docs/mcp-guide.md`

More detailed guide with:
- Step-by-step setup instructions
- Common MCP servers and their configuration
- Troubleshooting tips
- Advanced usage patterns

---

## 14. Open Questions & Decisions

### 1. Tool Name Collision

**Question**: If two MCP servers provide tools with the same name, how should we handle it?

**Options**:
- **Option A**: Prefix all MCP tools with server name (e.g., `github_search`, `jira_search`)
- **Option B**: Fail on collision with error message
- **Option C**: Use namespacing (e.g., `github::search`, `jira::search`)

**Decision**: **Option A** - Prefix with server name
- Matches OpenCode's approach
- Clear and unambiguous
- Easy to understand in tool calls

### 2. Lazy vs Eager Loading

**Question**: Should MCP servers be initialized on startup or on first use?

**Options**:
- **Eager**: Initialize all enabled servers on startup
- **Lazy**: Initialize servers when first tool is needed

**Decision**: **Eager loading** with `enabled: false` option
- Better UX (immediate feedback if server fails)
- Allows `hh mcp list` to show status
- Users can disable servers they don't need
- Add lazy loading later if startup time becomes an issue

### 3. Context Limit Management

**Question**: MCP tools can add significant context. Should we implement lazy tool discovery?

**Options**:
- **Full discovery**: Load all tool schemas on startup
- **Lazy discovery**: Only load schemas when server is mentioned
- **Tool search**: Implement search-based tool discovery

**Decision**: **Full discovery** initially
- Start simple
- Monitor context usage
- Add lazy discovery or tool search in Phase 4 if needed
- Follow OpenCode's approach: warn users about context-heavy servers

### 4. OAuth Token Storage

**Question**: Where should OAuth tokens be stored?

**Options**:
- `~/.config/hh/mcp-auth.json`
- `~/.local/share/hh/mcp-auth.json`
- System keychain

**Decision**: **`~/.local/share/hh/mcp-auth.json`**
- Matches OpenCode's approach
- Follows XDG Base Directory Specification
- Separate from config (data vs configuration)
- Add keychain support later for better security

### 5. Error Messages

**Question**: How detailed should error messages be?

**Decision**: **Detailed with actionable suggestions**
- Include server name, command/URL, and error
- Suggest common fixes (e.g., "Check if npx is installed")
- Link to documentation
- Log full error for debugging

### 6. Concurrency

**Question**: Should multiple tool calls to the same MCP server be concurrent?

**Decision**: **Yes, but with limits**
- Allow concurrent tool calls
- Implement rate limiting per server
- Add configurable max concurrent calls (default: 5)

---

## 15. Estimated Effort

### Phase 1: Foundation
- Configuration schema: 4 hours
- MCP client manager: 8 hours
- Stdio transport: 8 hours
- **Total**: 20 hours (2-3 days)

### Phase 2: Tool Integration
- MCP tool wrapper: 4 hours
- Registry integration: 4 hours
- Testing and debugging: 8 hours
- **Total**: 16 hours (2 days)

### Phase 3: Permissions & UX
- Permission system: 6 hours
- CLI commands: 6 hours
- Testing: 4 hours
- **Total**: 16 hours (2 days)

### Phase 4: Advanced Features
- HTTP transport: 8 hours
- OAuth support: 12 hours
- System prompt injection: 4 hours
- **Total**: 24 hours (3 days)

### Overall Total: 76 hours (9-10 days)

With buffer for unexpected issues: **10-12 days**

---

## 16. Success Criteria

### Must Have (MVP)
- [ ] MCP servers can be configured in `hh.json`
- [ ] Local (stdio) MCP servers connect successfully
- [ ] MCP tools appear in tool schemas
- [ ] MCP tools can be executed by the agent
- [ ] Tool results are correctly formatted
- [ ] Basic error handling works
- [ ] `hh mcp list` command works

### Should Have
- [ ] HTTP/SSE transport support
- [ ] Permission system for MCP tools
- [ ] Environment variable substitution in config
- [ ] Graceful degradation on server failure
- [ ] Timeout enforcement

### Nice to Have
- [ ] OAuth authentication
- [ ] System prompt metadata injection
- [ ] Connection health monitoring
- [ ] Retry logic with exponential backoff
- [ ] Tool usage analytics

---

## 17. Risks & Mitigations

### Risk 1: MCP Protocol Changes
- **Risk**: MCP specification changes, breaking compatibility
- **Mitigation**: Use official SDK (`rmcp`), pin version, monitor spec updates

### Risk 2: Server Instability
- **Risk**: Third-party MCP servers crash or hang
- **Mitigation**: Timeouts, graceful degradation, process isolation

### Risk 3: Context Explosion
- **Risk**: Too many MCP tools exceed context limit
- **Mitigation**: Warn users, implement lazy loading, allow tool filtering

### Risk 4: Security Concerns
- **Risk**: Malicious MCP servers or tools
- **Mitigation**: Permission system, user approval, sandboxing (future)

### Risk 5: Performance Impact
- **Risk**: MCP server initialization slows startup
- **Mitigation**: Parallel initialization, lazy loading option, timeout

---

## 18. Future Enhancements

### Post-MVP Features

1. **MCP Resources Support**
   - Read resources from MCP servers
   - Resource templates
   - Resource subscriptions

2. **MCP Prompts Support**
   - Server-provided prompt templates
   - Dynamic prompt generation

3. **Advanced Tool Search**
   - Semantic search over tool descriptions
   - Lazy loading based on relevance

4. **Sandboxing**
   - Run MCP servers in isolated environments
   - Restrict file system access
   - Network isolation

5. **Tool Composition**
   - Chain multiple MCP tools
   - Workflow automation

6. **Analytics & Monitoring**
   - Track MCP tool usage
   - Performance metrics
   - Cost tracking

7. **MCP Server Management**
   - Install/uninstall MCP servers from CLI
   - Server marketplace/registry
   - Version management

---

## 19. References

### MCP Specification
- [Model Context Protocol Specification](https://modelcontextprotocol.io/)
- [MCP TypeScript SDK](https://github.com/modelcontextprotocol/typescript-sdk)

### Rust Libraries
- [rmcp - Official Rust MCP SDK](https://github.com/modelcontextprotocol/rust-sdk)
- [rust-mcp-sdk](https://crates.io/crates/rust-mcp-sdk)

### Example MCP Servers
- [MCP Server Examples](https://github.com/modelcontextprotocol/servers)
- [server-everything (test server)](https://www.npmjs.com/package/@modelcontextprotocol/server-everything)

### OpenCode Implementation
- [OpenCode MCP Configuration](https://opencode.ai/docs/mcp-servers)
- [OpenCode Config Schema](https://opencode.ai/config.json)

### Claude Desktop
- [Claude Desktop MCP Setup](https://docs.anthropic.com/claude/docs/mcp)

---

## 20. Appendix: Configuration Examples

### Example 1: Local Filesystem Server

```json
{
  "mcp": {
    "servers": {
      "fs": {
        "type": "local",
        "command": [
          "npx",
          "-y",
          "@modelcontextprotocol/server-filesystem",
          "/home/user/projects"
        ],
        "enabled": true
      }
    }
  }
}
```

### Example 2: GitHub Server with Authentication

```json
{
  "mcp": {
    "servers": {
      "github": {
        "type": "local",
        "command": ["npx", "-y", "@modelcontextprotocol/server-github"],
        "environment": {
          "GITHUB_TOKEN": "{env:GITHUB_TOKEN}"
        },
        "enabled": true
      }
    }
  },
  "permissions": {
    "mcp": {
      "tools": {
        "github_*": "ask"
      }
    }
  }
}
```

### Example 3: Remote Server with OAuth

```json
{
  "mcp": {
    "servers": {
      "sentry": {
        "type": "remote",
        "url": "https://mcp.sentry.dev/mcp",
        "oauth": {
          "scope": "org:read project:read"
        },
        "enabled": true
      }
    }
  }
}
```

### Example 4: Multiple Servers with Permissions

```json
{
  "mcp": {
    "servers": {
      "filesystem": {
        "type": "local",
        "command": ["npx", "-y", "@modelcontextprotocol/server-filesystem", "."],
        "enabled": true
      },
      "postgres": {
        "type": "local",
        "command": ["npx", "-y", "@modelcontextprotocol/server-postgres"],
        "environment": {
          "DATABASE_URL": "{env:DATABASE_URL}"
        },
        "enabled": false
      },
      "context7": {
        "type": "remote",
        "url": "https://mcp.context7.com/mcp",
        "headers": {
          "CONTEXT7_API_KEY": "{env:CONTEXT7_API_KEY}"
        },
        "enabled": true
      }
    }
  },
  "permissions": {
    "mcp": {
      "servers": {
        "filesystem": "allow",
        "postgres": "deny"
      },
      "tools": {
        "filesystem_write*": "ask",
        "context7_*": "allow"
      }
    }
  }
}
```

---

## Conclusion

This plan provides a comprehensive roadmap for implementing MCP server support in the `hh` agent runtime. The implementation follows industry standards (matching OpenCode and Claude Desktop), uses the official Rust MCP SDK, and integrates cleanly with the existing architecture.

The phased approach allows for incremental delivery:
- **Phase 1-2**: MVP with local server support
- **Phase 3**: Enhanced UX and permissions
- **Phase 4**: Advanced features (HTTP, OAuth)

Key design principles:
1. **Compatibility**: Match OpenCode/Claude configuration schema
2. **Safety**: Permission system and approval workflow
3. **Reliability**: Graceful degradation and error handling
4. **Extensibility**: Easy to add new transports and features
5. **User Experience**: Clear CLI commands and helpful error messages

Start with Phase 1 to establish the foundation, then iterate based on user feedback and real-world usage.
