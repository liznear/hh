package tools

import (
	"context"
	"fmt"
	"os"
	"path/filepath"

	"github.com/liznear/hh/agent"
)

type WriteResult struct {
	Path        string
	OldContent  string
	NewContent  string
	AddedLines  int
}

func (r WriteResult) Summary() string {
	if r.AddedLines == 0 {
		return "no changes"
	}
	return fmt.Sprintf("+%d", r.AddedLines)
}

func NewWriteTool() agent.Tool {
	return agent.Tool{
		Name:        "write",
		Description: "Write full content to a file",
		Schema: map[string]any{
			"type": "object",
			"properties": map[string]any{
				"path":    map[string]any{"type": "string"},
				"content": map[string]any{"type": "string"},
			},
			"required": []string{"path", "content"},
		},
		Handler: agent.FuncToolHandler(handleWrite),
	}
}

func handleWrite(_ context.Context, params map[string]any) agent.ToolResult {
	path, err := requiredString(params, "path")
	if err != nil {
		return toolErr("%s", err.Error())
	}
	content, err := optionalString(params, "content")
	if err != nil {
		return toolErr("%s", err.Error())
	}

	original, readErr := os.ReadFile(path)
	if readErr != nil && !os.IsNotExist(readErr) {
		return toolErr("failed to read file: %v", readErr)
	}

	mode := os.FileMode(0o644)
	if info, statErr := os.Stat(path); statErr == nil {
		mode = info.Mode()
	}

	parent := filepath.Dir(path)
	if parent != "." {
		if err := os.MkdirAll(parent, 0o755); err != nil {
			return toolErr("failed to create parent directory: %v", err)
		}
	}

	if err := os.WriteFile(path, []byte(content), mode); err != nil {
		return toolErr("failed to write file: %v", err)
	}

	oldContent := string(original)
	addedLines, _ := countDiffChanges(oldContent, content)

	return agent.ToolResult{
		Data: "ok",
		Result: WriteResult{
			Path:       path,
			OldContent: oldContent,
			NewContent: content,
			AddedLines: addedLines,
		},
	}
}
