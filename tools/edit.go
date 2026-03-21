package tools

import (
	"context"
	"fmt"
	"os"
	"strings"

	"github.com/liznear/hh/agent"
)

type EditResult struct {
	Path             string
	ReplacementCount int
}

func (r EditResult) Summary() string {
	if r.ReplacementCount == 0 {
		return "no changes"
	}
	return fmt.Sprintf("%d replacements", r.ReplacementCount)
}

func NewEditTool() agent.Tool {
	return agent.Tool{
		Name:        "edit",
		Description: "Replace text in a file",
		Schema: map[string]any{
			"type": "object",
			"properties": map[string]any{
				"path":       map[string]any{"type": "string"},
				"old_string": map[string]any{"type": "string"},
				"new_string": map[string]any{"type": "string"},
			},
			"required": []string{"path", "old_string", "new_string"},
		},
		Handler: agent.FuncToolHandler(handleEdit),
	}
}

func handleEdit(_ context.Context, params map[string]any) agent.ToolResult {
	path, err := requiredString(params, "path")
	if err != nil {
		return toolErr("%s", err.Error())
	}
	oldString, err := requiredString(params, "old_string")
	if err != nil {
		return toolErr("%s", err.Error())
	}
	newString, err := optionalString(params, "new_string")
	if err != nil {
		return toolErr("%s", err.Error())
	}

	if oldString == "" {
		return toolErr("old_string must not be empty")
	}

	original, err := os.ReadFile(path)
	if err != nil {
		return toolErr("failed to read file: %v", err)
	}

	content := string(original)
	if !strings.Contains(content, oldString) {
		return toolErr("old_string not found")
	}

	updated := strings.ReplaceAll(content, oldString, newString)
	replacementCount := strings.Count(content, oldString)
	info, err := os.Stat(path)
	if err != nil {
		return toolErr("failed to stat file: %v", err)
	}
	if err := os.WriteFile(path, []byte(updated), info.Mode()); err != nil {
		return toolErr("failed to write file: %v", err)
	}

	return agent.ToolResult{
		Data: "ok",
		Result: EditResult{
			Path:             path,
			ReplacementCount: replacementCount,
		},
	}
}
