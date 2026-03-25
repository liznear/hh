package tools

import (
	"context"
	"os"
	"path/filepath"
	"testing"
)

func TestReadTool(t *testing.T) {
	tmpDir := t.TempDir()
	path := filepath.Join(tmpDir, "sample.txt")
	content := "line0\nline1\nline2\nline3\n"
	if err := os.WriteFile(path, []byte(content), 0o644); err != nil {
		t.Fatalf("failed to prepare file: %v", err)
	}

	args := map[string]any{
		"path":  path,
		"start": 1,
		"limit": 2,
	}

	res := NewReadTool().Handler.Handle(context.Background(), args)
	if res.IsErr {
		t.Fatalf("expected success, got error: %s", res.Data)
	}
	if res.Data != "line1\nline2" {
		t.Fatalf("unexpected read output: %q", res.Data)
	}

	structured, ok := res.Result.(ReadResult)
	if !ok {
		t.Fatalf("unexpected result type: %T", res.Result)
	}
	if structured.LineCount != 2 {
		t.Fatalf("unexpected line count: %d", structured.LineCount)
	}
}

func TestReadToolOptionalParams(t *testing.T) {
	tmpDir := t.TempDir()
	path := filepath.Join(tmpDir, "sample.txt")
	content := "line0\nline1\nline2\nline3\n"
	if err := os.WriteFile(path, []byte(content), 0o644); err != nil {
		t.Fatalf("failed to prepare file: %v", err)
	}

	// Test: no start/limit provided (should read all from start 0)
	t.Run("defaults", func(t *testing.T) {
		args := map[string]any{"path": path}
		res := NewReadTool().Handler.Handle(context.Background(), args)
		if res.IsErr {
			t.Fatalf("expected success, got error: %s", res.Data)
		}
		if res.Data != "line0\nline1\nline2\nline3" {
			t.Fatalf("unexpected read output: %q", res.Data)
		}
	})

	// Test: only start provided (should read all from that start)
	t.Run("start_only", func(t *testing.T) {
		args := map[string]any{"path": path, "start": 2}
		res := NewReadTool().Handler.Handle(context.Background(), args)
		if res.IsErr {
			t.Fatalf("expected success, got error: %s", res.Data)
		}
		if res.Data != "line2\nline3" {
			t.Fatalf("unexpected read output: %q", res.Data)
		}
	})

	// Test: only limit provided (should read that many from start 0)
	t.Run("limit_only", func(t *testing.T) {
		args := map[string]any{"path": path, "limit": 2}
		res := NewReadTool().Handler.Handle(context.Background(), args)
		if res.IsErr {
			t.Fatalf("expected success, got error: %s", res.Data)
		}
		if res.Data != "line0\nline1" {
			t.Fatalf("unexpected read output: %q", res.Data)
		}
	})
}
