# Agent Markdown Definitions Implementation Plan

**Goal:** Enable defining agents via markdown files with frontmatter that are embedded in the binary, replacing the hardcoded agent map.

**Architecture:** 
- Create `agents/` directory containing markdown files, one per agent
- Each markdown file has YAML frontmatter with `name` and `allowed_tools` fields
- Create `agents/loader.go` package that parses these files and returns Agent structs
- Use Go's `embed` package to embed the `agents/` directory in the binary
- Update `tui/agents.go` to load from the embedded catalog instead of the hardcoded map
- Follow the existing pattern from `skills/catalog.go` for consistency

**Final Completion Criteria:**
- [ ] Agents defined in markdown files under `agents/` directory
- [ ] Files embedded in binary using `//go:embed`
- [ ] `tui/agents.go` loads agents from embedded catalog
- [ ] Existing "Build" agent migrated to markdown format
- [ ] Tests verify parsing and loading work correctly
- [ ] All existing tests pass

---

## File Structure

```
agents/
  build.md           # Build agent definition
  loader.go          # Parsing and loading logic
  loader_test.go     # Unit tests for parsing

tui/
  agents.go          # Updated to use agents.Catalog
  agents_test.go     # Updated tests
```

---

### Task 1: Create Agent Loader Package

**Files:**
- Create: `agents/loader.go`
- Create: `agents/loader_test.go`

- [ ] **Step 1: Write the Agent struct and parsing logic**

Create `agents/loader.go`:

```go
package agents

import (
	"embed"
	"fmt"
	"sort"
	"strings"
	"sync"
)

//go:embed *.md
var agentFiles embed.FS

type Agent struct {
	Name         string
	SystemPrompt string
	AllowedTools []string
}

type Catalog struct {
	agents map[string]Agent
}

func (c Catalog) Get(name string) (Agent, bool) {
	agent, ok := c.agents[name]
	return agent, ok
}

func (c Catalog) All() []Agent {
	result := make([]Agent, 0, len(c.agents))
	for _, agent := range c.agents {
		result = append(result, agent)
	}
	sort.Slice(result, func(i, j int) bool {
		return result[i].Name < result[j].Name
	})
	return result
}

var (
	defaultCatalogOnce sync.Once
	defaultCatalog     Catalog
	defaultCatalogErr  error
)

func LoadDefaultCatalog() (Catalog, error) {
	defaultCatalogOnce.Do(func() {
		defaultCatalog, defaultCatalogErr = loadFromEmbed()
	})
	return defaultCatalog, defaultCatalogErr
}

func loadFromEmbed() (Catalog, error) {
	entries, err := agentFiles.ReadDir(".")
	if err != nil {
		return Catalog{}, fmt.Errorf("read embedded agents dir: %w", err)
	}

	agentsMap := make(map[string]Agent)
	for _, entry := range entries {
		if entry.IsDir() || !strings.HasSuffix(entry.Name(), ".md") {
			continue
		}

		content, err := agentFiles.ReadFile(entry.Name())
		if err != nil {
			return Catalog{}, fmt.Errorf("read agent file %s: %w", entry.Name(), err)
		}

		agent, err := parseAgentMarkdown(string(content))
		if err != nil {
			return Catalog{}, fmt.Errorf("parse agent file %s: %w", entry.Name(), err)
		}

		if agent.Name == "" {
			return Catalog{}, fmt.Errorf("agent file %s missing name field", entry.Name())
		}

		agentsMap[agent.Name] = agent
	}

	return Catalog{agents: agentsMap}, nil
}

func parseAgentMarkdown(content string) (Agent, error) {
	frontmatter, body := extractFrontmatter(content)
	fields := parseFrontmatterFields(frontmatter)

	name := strings.TrimSpace(fields["name"])
	if name == "" {
		return Agent{}, fmt.Errorf("missing required field: name")
	}

	var allowedTools []string
	if toolsStr := strings.TrimSpace(fields["allowed_tools"]); toolsStr != "" {
		// Parse as comma-separated list
		for _, tool := range strings.Split(toolsStr, ",") {
			tool = strings.TrimSpace(tool)
			if tool != "" {
				allowedTools = append(allowedTools, tool)
			}
		}
	}

	return Agent{
		Name:         name,
		SystemPrompt: strings.TrimSpace(body),
		AllowedTools: allowedTools,
	}, nil
}

func extractFrontmatter(content string) (string, string) {
	content = strings.TrimSpace(content)
	if !strings.HasPrefix(content, "---\n") && !strings.HasPrefix(content, "---\r\n") {
		return "", content
	}

	startDelimiterLen := len("---\n")
	if strings.HasPrefix(content, "---\r\n") {
		startDelimiterLen = len("---\r\n")
	}

	rest := content[startDelimiterLen:]
	end := strings.Index(rest, "\n---")
	if end < 0 {
		return "", content
	}

	frontmatter := strings.TrimSpace(rest[:end])
	body := strings.TrimSpace(rest[end+4:])
	return frontmatter, body
}

func parseFrontmatterFields(raw string) map[string]string {
	out := map[string]string{}
	if strings.TrimSpace(raw) == "" {
		return out
	}

	for _, line := range strings.Split(raw, "\n") {
		line = strings.TrimSpace(line)
		if line == "" || strings.HasPrefix(line, "#") {
			continue
		}
		key, value, ok := strings.Cut(line, ":")
		if !ok {
			continue
		}
		key = strings.ToLower(strings.TrimSpace(key))
		if key == "" {
			continue
		}
		value = strings.TrimSpace(value)
		value = strings.Trim(value, "\"'")
		out[key] = value
	}

	return out
}
```

- [ ] **Step 2: Write unit tests for the loader**

Create `agents/loader_test.go`:

```go
package agents

import (
	"strings"
	"testing"
)

func TestParseAgentMarkdown_Basic(t *testing.T) {
	content := `---
name: Build
allowed_tools: Bash, Read, Write
---
You are Build, a software engineering agent.`

	agent, err := parseAgentMarkdown(content)
	if err != nil {
		t.Fatalf("unexpected error: %v", err)
	}
	if agent.Name != "Build" {
		t.Errorf("expected name Build, got %q", agent.Name)
	}
	if agent.SystemPrompt != "You are Build, a software engineering agent." {
		t.Errorf("unexpected system prompt: %q", agent.SystemPrompt)
	}
	if len(agent.AllowedTools) != 3 {
		t.Errorf("expected 3 tools, got %d", len(agent.AllowedTools))
	}
}

func TestParseAgentMarkdown_NoTools(t *testing.T) {
	content := `---
name: Build
---
You are Build.`

	agent, err := parseAgentMarkdown(content)
	if err != nil {
		t.Fatalf("unexpected error: %v", err)
	}
	if len(agent.AllowedTools) != 0 {
		t.Errorf("expected 0 tools, got %d", len(agent.AllowedTools))
	}
}

func TestParseAgentMarkdown_MissingName(t *testing.T) {
	content := `---
allowed_tools: Bash
---
You are an agent.`

	_, err := parseAgentMarkdown(content)
	if err == nil {
		t.Fatal("expected error for missing name")
	}
	if !strings.Contains(err.Error(), "missing required field: name") {
		t.Errorf("unexpected error: %v", err)
	}
}

func TestParseAgentMarkdown_NoFrontmatter(t *testing.T) {
	content := `You are an agent without frontmatter.`

	_, err := parseAgentMarkdown(content)
	if err == nil {
		t.Fatal("expected error for missing frontmatter")
	}
}

func TestExtractFrontmatter(t *testing.T) {
	tests := []struct {
		name              string
		content           string
		wantFrontmatter   string
		wantBody          string
	}{
		{
			name: "basic frontmatter",
			content: "---\nname: Test\n---\nBody content",
			wantFrontmatter: "name: Test",
			wantBody:        "Body content",
		},
		{
			name: "no frontmatter",
			content: "Just body",
			wantFrontmatter: "",
			wantBody:        "Just body",
		},
		{
			name: "multiline body",
			content: "---\nname: Test\n---\nLine 1\nLine 2",
			wantFrontmatter: "name: Test",
			wantBody:        "Line 1\nLine 2",
		},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			fm, body := extractFrontmatter(tt.content)
			if fm != tt.wantFrontmatter {
				t.Errorf("frontmatter: got %q, want %q", fm, tt.wantFrontmatter)
			}
			if body != tt.wantBody {
				t.Errorf("body: got %q, want %q", body, tt.wantBody)
			}
		})
	}
}
```

- [ ] **Step 3: Run tests to verify parsing logic**

Run: `go test ./agents/... -v`
Expected: All tests pass

- [ ] **Step 4: Commit the loader package**

```bash
git add agents/loader.go agents/loader_test.go
git commit -m "feat: add agents loader package with markdown parsing"
```

---

### Task 2: Create Build Agent Markdown File

**Files:**
- Create: `agents/build.md`

- [ ] **Step 1: Create the Build agent markdown file**

Create `agents/build.md`:

```markdown
---
name: Build
---
You are Build, a software engineering agent focused on making correct, maintainable code changes.
```

- [ ] **Step 2: Verify embedding works**

Run: `go test ./agents/... -v -run TestLoadDefaultCatalog`
Expected: Test passes (will create this test in next task)

- [ ] **Step 3: Commit the Build agent definition**

```bash
git add agents/build.md
git commit -m "feat: add Build agent markdown definition"
```

---

### Task 3: Add Integration Test for Embedded Catalog

**Files:**
- Modify: `agents/loader_test.go`

- [ ] **Step 1: Add test for loading embedded catalog**

Add to `agents/loader_test.go`:

```go
func TestLoadDefaultCatalog(t *testing.T) {
	catalog, err := LoadDefaultCatalog()
	if err != nil {
		t.Fatalf("unexpected error: %v", err)
	}

	buildAgent, ok := catalog.Get("Build")
	if !ok {
		t.Fatal("expected Build agent in catalog")
	}

	if buildAgent.Name != "Build" {
		t.Errorf("expected name Build, got %q", buildAgent.Name)
	}

	if buildAgent.SystemPrompt == "" {
		t.Error("expected non-empty system prompt")
	}
}

func TestCatalogGet_NotFound(t *testing.T) {
	catalog, err := LoadDefaultCatalog()
	if err != nil {
		t.Fatalf("unexpected error: %v", err)
	}

	_, ok := catalog.Get("NonExistent")
	if ok {
		t.Error("expected false for non-existent agent")
	}
}

func TestCatalogAll(t *testing.T) {
	catalog, err := LoadDefaultCatalog()
	if err != nil {
		t.Fatalf("unexpected error: %v", err)
	}

	all := catalog.All()
	if len(all) == 0 {
		t.Error("expected at least one agent")
	}

	// Verify sorted by name
	for i := 1; i < len(all); i++ {
		if all[i-1].Name > all[i].Name {
			t.Errorf("agents not sorted: %s > %s", all[i-1].Name, all[i].Name)
		}
	}
}
```

- [ ] **Step 2: Run tests**

Run: `go test ./agents/... -v`
Expected: All tests pass

- [ ] **Step 3: Commit**

```bash
git add agents/loader_test.go
git commit -m "test: add integration tests for embedded agent catalog"
```

---

### Task 4: Update tui/agents.go to Use Catalog

**Files:**
- Modify: `tui/agents.go`
- Modify: `tui/agents_test.go`

- [ ] **Step 1: Update tui/agents.go**

Replace the hardcoded `agents` map and `getAgent` function:

```go
package tui

import (
	"fmt"

	"github.com/liznear/hh/agent"
	"github.com/liznear/hh/agents"
	"github.com/liznear/hh/config"
	"github.com/liznear/hh/skills"
	"github.com/liznear/hh/tools"
)

func newAgentRunner(modelName string, provider agent.Provider, agentName string, cfg config.Config, workingDir string) (*agent.AgentRunner, error) {
	agentConfig, err := getAgent(agentName)
	if err != nil {
		return nil, err
	}

	approver, err := newToolApprover(cfg, workingDir)
	if err != nil {
		return nil, err
	}

	skillCatalog, err := skills.LoadDefaultCatalog()
	if err != nil {
		return nil, err
	}
	tools.SetSkillCatalog(skillCatalog)
	systemPrompt := buildSystemPrompt(agentConfig.SystemPrompt, skillCatalog)

	return agent.NewAgentRunner(
		modelName,
		provider,
		agent.WithSystemPrompt(systemPrompt),
		agent.WithTools(resolveTools(agentConfig)),
		agent.WithToolApprover(approver),
	), nil
}

func buildSystemPrompt(base string, skillCatalog skills.Catalog) string {
	// ... keep existing implementation ...
}

func getAgent(name string) (agents.Agent, error) {
	if name == "" {
		name = "Build"
	}

	catalog, err := agents.LoadDefaultCatalog()
	if err != nil {
		return agents.Agent{}, fmt.Errorf("load agent catalog: %w", err)
	}

	agentConfig, ok := catalog.Get(name)
	if !ok {
		return agents.Agent{}, fmt.Errorf("agent %q not found", name)
	}

	return agentConfig, nil
}

func resolveTools(agentConfig agents.Agent) map[string]agent.Tool {
	if agentConfig.AllowedTools == nil {
		return tools.AllTools()
	}
	return tools.GetTools(agentConfig.AllowedTools)
}
```

- [ ] **Step 2: Update tui/agents_test.go**

Update imports and test to use new agents package:

```go
package tui

import (
	"os"
	"path/filepath"
	"strings"
	"testing"

	"github.com/liznear/hh/skills"
)

func TestGetAgent_DefaultBuild(t *testing.T) {
	agentConfig, err := getAgent("")
	if err != nil {
		t.Fatalf("expected default agent, got error: %v", err)
	}
	if agentConfig.Name != "Build" {
		t.Fatalf("expected default agent Build, got %q", agentConfig.Name)
	}
}

func TestGetAgent_NotFound(t *testing.T) {
	_, err := getAgent("missing")
	if err == nil {
		t.Fatalf("expected error for unknown agent")
	}
}

func TestBuildSystemPrompt_AppendsSkillsFrontmatter(t *testing.T) {
	root := t.TempDir()
	skillDir := filepath.Join(root, "cleanup")
	if err := os.MkdirAll(skillDir, 0o755); err != nil {
		t.Fatalf("mkdir skill dir: %v", err)
	}
	content := "---\nname: cleanup\ndescription: cleanup code\n---\n# Cleanup"
	if err := os.WriteFile(filepath.Join(skillDir, "SKILL.md"), []byte(content), 0o644); err != nil {
		t.Fatalf("write skill file: %v", err)
	}

	catalog, err := skills.LoadDir(root)
	if err != nil {
		t.Fatalf("load skills dir: %v", err)
	}

	got := buildSystemPrompt("base prompt", catalog)
	if !strings.Contains(got, "base prompt") {
		t.Fatalf("expected base prompt in result: %q", got)
	}
	if !strings.Contains(got, "<available_skills>") {
		t.Fatalf("expected available_skills block in result: %q", got)
	}
	if !strings.Contains(got, "<name>cleanup</name>") {
		t.Fatalf("expected cleanup skill in result: %q", got)
	}
}
```

- [ ] **Step 3: Run all tests**

Run: `go test ./tui/... -v`
Expected: All tests pass

- [ ] **Step 4: Run full test suite**

Run: `go test ./...`
Expected: All tests pass

- [ ] **Step 5: Commit**

```bash
git add tui/agents.go tui/agents_test.go
git commit -m "refactor: use embedded agent catalog instead of hardcoded map"
```

---

### Task 5: Verify End-to-End

- [ ] **Step 1: Build the binary**

Run: `go build -o hh .`
Expected: Build succeeds

- [ ] **Step 2: Run the application to verify Build agent loads**

Run: `./hh --help` or equivalent command to verify app starts
Expected: Application starts without errors

- [ ] **Step 3: Final commit (if any fixes needed)**

```bash
git add -A
git commit -m "fix: any remaining issues"
```

---

## Summary

This implementation:
1. Creates a new `agents` package with markdown parsing and embedding
2. Migrates the "Build" agent to `agents/build.md`
3. Updates `tui/agents.go` to use the new catalog
4. Maintains backward compatibility - all existing tests pass
5. Follows the established pattern from `skills/catalog.go`

The key insight is using `//go:embed *.md` to embed all markdown files in the `agents/` directory directly into the binary, making them available at runtime without external file dependencies.
