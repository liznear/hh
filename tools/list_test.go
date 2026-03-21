package tools

import (
	"context"
	"os"
	"path/filepath"
	"testing"
)

func TestListTool(t *testing.T) {
	tmpDir := t.TempDir()
	if err := os.WriteFile(filepath.Join(tmpDir, "a.txt"), []byte("a"), 0o644); err != nil {
		t.Fatalf("failed to prepare file: %v", err)
	}
	if err := os.Mkdir(filepath.Join(tmpDir, "dir"), 0o755); err != nil {
		t.Fatalf("failed to prepare dir: %v", err)
	}

	args := map[string]any{
		"path": tmpDir,
	}

	res := NewListTool().Handler.Handle(context.Background(), args)
	if res.IsErr {
		t.Fatalf("expected success, got error: %s", res.Data)
	}

	structured, ok := res.Result.(ListResult)
	if !ok {
		t.Fatalf("unexpected result type: %T", res.Result)
	}
	if structured.EntryCount != 2 || structured.FileCount != 1 || structured.DirCount != 1 {
		t.Fatalf("unexpected list summary: %+v", structured)
	}
}
