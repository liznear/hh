package tools

import (
	"context"
	"os"
	"os/exec"
	"path/filepath"
	"strings"
	"testing"
)

func TestGrepTool(t *testing.T) {
	if _, err := exec.LookPath("rg"); err != nil {
		t.Skip("rg is required for grep tool test")
	}

	tmpDir := t.TempDir()
	path := filepath.Join(tmpDir, "sample.txt")
	content := "alpha\nbeta\ngamma\n"
	if err := os.WriteFile(path, []byte(content), 0o644); err != nil {
		t.Fatalf("failed to prepare file: %v", err)
	}

	args := map[string]any{
		"pattern": "beta",
		"path":    path,
	}

	res := NewGrepTool().Handler.Handle(context.Background(), args)
	if res.IsErr {
		t.Fatalf("expected success, got error: %s", res.Data)
	}
	if !strings.Contains(res.Data, "sample.txt:2:beta") {
		t.Fatalf("unexpected grep output: %q", res.Data)
	}
}
