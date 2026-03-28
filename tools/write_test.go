package tools

import (
	"context"
	"os"
	"path/filepath"
	"testing"
)

func TestWriteTool_CreatesFile(t *testing.T) {
	tmpDir := t.TempDir()
	path := filepath.Join(tmpDir, "nested", "sample.txt")

	args := map[string]any{
		"path":    path,
		"content": "line1\nline2\n",
	}

	res := NewWriteTool().Handler.Handle(context.Background(), args)
	if res.IsErr {
		t.Fatalf("expected success, got error: %s", res.Data)
	}

	written, err := os.ReadFile(path)
	if err != nil {
		t.Fatalf("failed to read written file: %v", err)
	}
	if string(written) != "line1\nline2\n" {
		t.Fatalf("unexpected file content after write: %q", string(written))
	}

	structured, ok := res.Result.(WriteResult)
	if !ok {
		t.Fatalf("unexpected result type: %T", res.Result)
	}
	if structured.AddedLines != 2 {
		t.Fatalf("unexpected added line count: %+v", structured)
	}
	if structured.NewContent == "" {
		t.Fatalf("expected new content in write result")
	}
}
