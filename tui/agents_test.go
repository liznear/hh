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
