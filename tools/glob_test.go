package tools

import (
	"context"
	"os"
	"path/filepath"
	"strings"
	"testing"
)

func TestGlobTool(t *testing.T) {
	t.Run("matches files in provided path", func(t *testing.T) {
		tmpDir := t.TempDir()
		if err := os.WriteFile(filepath.Join(tmpDir, "a.txt"), []byte("a"), 0o644); err != nil {
			t.Fatalf("failed to prepare file: %v", err)
		}
		if err := os.WriteFile(filepath.Join(tmpDir, "b.txt"), []byte("b"), 0o644); err != nil {
			t.Fatalf("failed to prepare file: %v", err)
		}
		if err := os.WriteFile(filepath.Join(tmpDir, "c.go"), []byte("package main"), 0o644); err != nil {
			t.Fatalf("failed to prepare file: %v", err)
		}

		res := NewGlobTool().Handler.Handle(context.Background(), map[string]any{
			"pattern": "*.txt",
			"path":    tmpDir,
		})

		if res.IsErr {
			t.Fatalf("expected success, got error: %s", res.Data)
		}
		if res.Data != "a.txt\nb.txt" {
			t.Fatalf("unexpected glob output: %q", res.Data)
		}

		structured, ok := res.Result.(GlobResult)
		if !ok {
			t.Fatalf("unexpected result type: %T", res.Result)
		}
		if structured.Pattern != "*.txt" || structured.Path != tmpDir || structured.MatchCount != 2 {
			t.Fatalf("unexpected glob summary: %+v", structured)
		}
	})

	t.Run("uses current directory when path omitted", func(t *testing.T) {
		tmpDir := t.TempDir()
		if err := os.WriteFile(filepath.Join(tmpDir, "current.txt"), []byte("x"), 0o644); err != nil {
			t.Fatalf("failed to prepare file: %v", err)
		}

		wd, err := os.Getwd()
		if err != nil {
			t.Fatalf("failed to get wd: %v", err)
		}
		if err := os.Chdir(tmpDir); err != nil {
			t.Fatalf("failed to chdir: %v", err)
		}
		t.Cleanup(func() {
			_ = os.Chdir(wd)
		})

		res := NewGlobTool().Handler.Handle(context.Background(), map[string]any{
			"pattern": "*.txt",
		})

		if res.IsErr {
			t.Fatalf("expected success, got error: %s", res.Data)
		}
		if res.Data != "current.txt" {
			t.Fatalf("unexpected glob output: %q", res.Data)
		}

		structured, ok := res.Result.(GlobResult)
		if !ok {
			t.Fatalf("unexpected result type: %T", res.Result)
		}
		if structured.Path != "." || structured.MatchCount != 1 {
			t.Fatalf("unexpected glob summary: %+v", structured)
		}
	})

	t.Run("returns error for invalid pattern", func(t *testing.T) {
		res := NewGlobTool().Handler.Handle(context.Background(), map[string]any{
			"pattern": "[",
		})

		if !res.IsErr {
			t.Fatal("expected error for invalid pattern")
		}
		if !strings.Contains(res.Data, "invalid glob pattern") {
			t.Fatalf("unexpected error: %q", res.Data)
		}
	})

	t.Run("double star pattern matches recursively", func(t *testing.T) {
		tmpDir := t.TempDir()
		if err := os.MkdirAll(filepath.Join(tmpDir, "one", "two"), 0o755); err != nil {
			t.Fatalf("failed to prepare dirs: %v", err)
		}
		if err := os.WriteFile(filepath.Join(tmpDir, "one", "shallow.txt"), []byte("x"), 0o644); err != nil {
			t.Fatalf("failed to prepare file: %v", err)
		}
		if err := os.WriteFile(filepath.Join(tmpDir, "one", "two", "deep.txt"), []byte("x"), 0o644); err != nil {
			t.Fatalf("failed to prepare file: %v", err)
		}

		res := NewGlobTool().Handler.Handle(context.Background(), map[string]any{
			"pattern": "**/*.txt",
			"path":    tmpDir,
		})

		if res.IsErr {
			t.Fatalf("expected success, got error: %s", res.Data)
		}
		expected := strings.Join([]string{
			filepath.Join("one", "shallow.txt"),
			filepath.Join("one", "two", "deep.txt"),
		}, "\n")
		if res.Data != expected {
			t.Fatalf("unexpected double-star output: %q", res.Data)
		}
	})

	t.Run("returns error when path is not directory", func(t *testing.T) {
		tmpDir := t.TempDir()
		filePath := filepath.Join(tmpDir, "file.txt")
		if err := os.WriteFile(filePath, []byte("x"), 0o644); err != nil {
			t.Fatalf("failed to prepare file: %v", err)
		}

		res := NewGlobTool().Handler.Handle(context.Background(), map[string]any{
			"pattern": "*.txt",
			"path":    filePath,
		})

		if !res.IsErr {
			t.Fatal("expected error for non-directory path")
		}
		if !strings.Contains(res.Data, "path is not a directory") {
			t.Fatalf("unexpected error: %q", res.Data)
		}
	})

	t.Run("returns error when pattern is missing", func(t *testing.T) {
		res := NewGlobTool().Handler.Handle(context.Background(), map[string]any{})
		if !res.IsErr {
			t.Fatal("expected error when pattern missing")
		}
		if !strings.Contains(res.Data, "pattern is required") {
			t.Fatalf("unexpected error: %q", res.Data)
		}
	})
}
