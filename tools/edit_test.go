package tools

import (
	"context"
	"os"
	"path/filepath"
	"testing"
)

func TestEditTool(t *testing.T) {
	tmpDir := t.TempDir()
	path := filepath.Join(tmpDir, "sample.txt")
	content := "hello world\nhello world\n"
	if err := os.WriteFile(path, []byte(content), 0o644); err != nil {
		t.Fatalf("failed to prepare file: %v", err)
	}

	args := map[string]any{
		"path":       path,
		"old_string": "hello",
		"new_string": "hi",
	}

	res := NewEditTool().Handler.Handle(context.Background(), args)
	if res.IsErr {
		t.Fatalf("expected success, got error: %s", res.Data)
	}

	updated, err := os.ReadFile(path)
	if err != nil {
		t.Fatalf("failed to read updated file: %v", err)
	}
	if string(updated) != "hi world\nhi world\n" {
		t.Fatalf("unexpected file content after edit: %q", string(updated))
	}

	structured, ok := res.Result.(EditResult)
	if !ok {
		t.Fatalf("unexpected result type: %T", res.Result)
	}
	if structured.AddedLines != 2 || structured.DeletedLines != 2 {
		t.Fatalf("unexpected edit counts: %+v", structured)
	}
	if structured.OldContent == "" || structured.NewContent == "" {
		t.Fatalf("expected old and new content in edit result")
	}
}
