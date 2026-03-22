package tools

import (
	"context"
	"os"
	"strings"
	"testing"

	"github.com/liznear/hh/skills"
)

func TestSkillTool_LoadsSkillContent(t *testing.T) {
	SetSkillCatalog(skills.Catalog{})
	defer SetSkillCatalog(skills.Catalog{})

	root := t.TempDir()
	t.Setenv("HOME", root)

	skillDir := root + "/.agents/skills/demo"
	if err := os.MkdirAll(skillDir, 0o755); err != nil {
		t.Fatalf("mkdir skill dir: %v", err)
	}
	content := "---\nname: demo\ndescription: demo skill\n---\n# Demo\nUse this"
	if err := os.WriteFile(skillDir+"/SKILL.md", []byte(content), 0o644); err != nil {
		t.Fatalf("write skill file: %v", err)
	}

	// Force cache refresh from the temporary HOME.
	catalog, err := skills.LoadDir(root + "/.agents/skills")
	if err != nil {
		t.Fatalf("load temp catalog: %v", err)
	}
	SetSkillCatalog(catalog)

	res := NewSkillTool().Handler.Handle(context.Background(), map[string]any{"name": "demo"})
	if res.IsErr {
		t.Fatalf("expected success, got error: %s", res.Data)
	}

	if res.Data == "" || res.Data == "{}" {
		t.Fatalf("expected skill content payload, got %q", res.Data)
	}
	if !strings.HasPrefix(res.Data, "<skill_content") {
		t.Fatalf("unexpected skill payload: %q", res.Data)
	}

	structured, ok := res.Result.(SkillResult)
	if !ok {
		t.Fatalf("unexpected result type: %T", res.Result)
	}
	if structured.Name != "demo" {
		t.Fatalf("skill name = %q, want %q", structured.Name, "demo")
	}
}
