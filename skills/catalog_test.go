package skills

import (
	"os"
	"path/filepath"
	"strings"
	"testing"
)

func TestLoadDir_LoadsSkillEntriesAndFrontmatter(t *testing.T) {
	root := t.TempDir()
	skillDir := filepath.Join(root, "cleanup")
	if err := os.MkdirAll(skillDir, 0o755); err != nil {
		t.Fatalf("mkdir skill dir: %v", err)
	}

	skillContent := "---\nname: cleanup\ndescription: remove dead code\n---\n# Cleanup\nDo X"
	if err := os.WriteFile(filepath.Join(skillDir, skillFileName), []byte(skillContent), 0o644); err != nil {
		t.Fatalf("write skill file: %v", err)
	}

	catalog, err := LoadDir(root)
	if err != nil {
		t.Fatalf("LoadDir() error = %v", err)
	}

	if catalog.IsEmpty() {
		t.Fatal("expected non-empty catalog")
	}

	entry, ok := catalog.SkillByName("cleanup")
	if !ok {
		t.Fatal("expected skill named cleanup")
	}
	if entry.Description != "remove dead code" {
		t.Fatalf("entry description = %q, want %q", entry.Description, "remove dead code")
	}
	if !strings.Contains(entry.Frontmatter, "name: cleanup") {
		t.Fatalf("expected frontmatter to include name, got %q", entry.Frontmatter)
	}

	block := catalog.PromptFrontmatterBlock()
	if !strings.Contains(block, "<available_skills>") {
		t.Fatalf("prompt block missing available_skills tag: %q", block)
	}
	if !strings.Contains(block, "<name>cleanup</name>") {
		t.Fatalf("prompt block missing skill name: %q", block)
	}
	if !strings.Contains(block, "<frontmatter>") {
		t.Fatalf("prompt block missing frontmatter tag: %q", block)
	}
}
