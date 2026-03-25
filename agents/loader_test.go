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
		name            string
		content         string
		wantFrontmatter string
		wantBody        string
	}{
		{
			name:            "basic frontmatter",
			content:         "---\nname: Test\n---\nBody content",
			wantFrontmatter: "name: Test",
			wantBody:        "Body content",
		},
		{
			name:            "no frontmatter",
			content:         "Just body",
			wantFrontmatter: "",
			wantBody:        "Just body",
		},
		{
			name:            "multiline body",
			content:         "---\nname: Test\n---\nLine 1\nLine 2",
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
