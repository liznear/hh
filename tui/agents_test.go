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

	got := buildSystemPrompt("base prompt", catalog, "")
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

func TestBuildSystemPrompt_InjectsGlobalAgentsMD(t *testing.T) {
	root := t.TempDir()
	globalAgentsPath := filepath.Join(root, ".agents", "AGENTS.md")
	if err := os.MkdirAll(filepath.Dir(globalAgentsPath), 0o755); err != nil {
		t.Fatalf("mkdir global agents dir: %v", err)
	}
	if err := os.WriteFile(globalAgentsPath, []byte("global rules"), 0o644); err != nil {
		t.Fatalf("write global agents md: %v", err)
	}

	// Create empty project directory
	projectDir := filepath.Join(root, "project")
	if err := os.MkdirAll(projectDir, 0o755); err != nil {
		t.Fatalf("mkdir project dir: %v", err)
	}

	// Test helper that uses custom global path
	originalGetGlobalPath := getGlobalAgentsMDPath
	defer func() { getGlobalAgentsMDPath = originalGetGlobalPath }()
	getGlobalAgentsMDPath = func() string { return globalAgentsPath }

	got := buildSystemPrompt("base prompt", skills.Catalog{}, projectDir)
	if !strings.Contains(got, "base prompt") {
		t.Fatalf("expected base prompt in result: %q", got)
	}
	if !strings.Contains(got, "<global-agents-md>") {
		t.Fatalf("expected global-agents-md block in result: %q", got)
	}
	if !strings.Contains(got, "global rules") {
		t.Fatalf("expected global rules content in result: %q", got)
	}
}

func TestBuildSystemPrompt_InjectsProjectAgentsMD(t *testing.T) {
	root := t.TempDir()
	projectDir := filepath.Join(root, "project")
	if err := os.MkdirAll(projectDir, 0o755); err != nil {
		t.Fatalf("mkdir project dir: %v", err)
	}
	projectAgentsPath := filepath.Join(projectDir, "AGENTS.md")
	if err := os.WriteFile(projectAgentsPath, []byte("project rules"), 0o644); err != nil {
		t.Fatalf("write project agents md: %v", err)
	}

	got := buildSystemPrompt("base prompt", skills.Catalog{}, projectDir)
	if !strings.Contains(got, "base prompt") {
		t.Fatalf("expected base prompt in result: %q", got)
	}
	if !strings.Contains(got, "<project-agents-md>") {
		t.Fatalf("expected project-agents-md block in result: %q", got)
	}
	if !strings.Contains(got, "project rules") {
		t.Fatalf("expected project rules content in result: %q", got)
	}
}

func TestBuildSystemPrompt_InjectsBothAgentsMD(t *testing.T) {
	root := t.TempDir()

	// Create global AGENTS.md
	globalAgentsPath := filepath.Join(root, ".agents", "AGENTS.md")
	if err := os.MkdirAll(filepath.Dir(globalAgentsPath), 0o755); err != nil {
		t.Fatalf("mkdir global agents dir: %v", err)
	}
	if err := os.WriteFile(globalAgentsPath, []byte("global rules"), 0o644); err != nil {
		t.Fatalf("write global agents md: %v", err)
	}

	// Create project AGENTS.md
	projectDir := filepath.Join(root, "project")
	if err := os.MkdirAll(projectDir, 0o755); err != nil {
		t.Fatalf("mkdir project dir: %v", err)
	}
	projectAgentsPath := filepath.Join(projectDir, "AGENTS.md")
	if err := os.WriteFile(projectAgentsPath, []byte("project rules"), 0o644); err != nil {
		t.Fatalf("write project agents md: %v", err)
	}

	// Override global path
	originalGetGlobalPath := getGlobalAgentsMDPath
	defer func() { getGlobalAgentsMDPath = originalGetGlobalPath }()
	getGlobalAgentsMDPath = func() string { return globalAgentsPath }

	got := buildSystemPrompt("base prompt", skills.Catalog{}, projectDir)
	if !strings.Contains(got, "<global-agents-md>") {
		t.Fatalf("expected global-agents-md block in result: %q", got)
	}
	if !strings.Contains(got, "<project-agents-md>") {
		t.Fatalf("expected project-agents-md block in result: %q", got)
	}
	if !strings.Contains(got, "global rules") {
		t.Fatalf("expected global rules content in result: %q", got)
	}
	if !strings.Contains(got, "project rules") {
		t.Fatalf("expected project rules content in result: %q", got)
	}
}
