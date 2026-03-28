package tools

import (
	"context"
	"fmt"
	"os"
	"strings"

	"github.com/liznear/hh/agent"
)

type EditResult struct {
	Path         string
	OldContent   string
	NewContent   string
	AddedLines   int
	DeletedLines int
}

func (r EditResult) Summary() string {
	if r.AddedLines == 0 && r.DeletedLines == 0 {
		return "no changes"
	}
	return fmt.Sprintf("+%d -%d", r.AddedLines, r.DeletedLines)
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
	info, err := os.Stat(path)
	if err != nil {
		return toolErr("failed to stat file: %v", err)
	}
	if err := os.WriteFile(path, []byte(updated), info.Mode()); err != nil {
		return toolErr("failed to write file: %v", err)
	}

	addedLines, deletedLines := countDiffChanges(content, updated)

	return agent.ToolResult{
		Data: "ok",
		Result: EditResult{
			Path:         path,
			OldContent:   content,
			NewContent:   updated,
			AddedLines:   addedLines,
			DeletedLines: deletedLines,
		},
	}
}

func countDiffChanges(oldContent, newContent string) (added int, deleted int) {
	if oldContent == newContent {
		return 0, 0
	}

	oldLines := strings.Split(oldContent, "\n")
	newLines := strings.Split(newContent, "\n")

	oldSet := make(map[string]int)
	for _, line := range oldLines {
		oldSet[line]++
	}

	newSet := make(map[string]int)
	for _, line := range newLines {
		newSet[line]++
	}

	for line, count := range newSet {
		if oldCount := oldSet[line]; oldCount < count {
			added += count - oldCount
		}
	}

	for line, count := range oldSet {
		if newCount := newSet[line]; newCount < count {
			deleted += count - newCount
		}
	}

	return added, deleted
}
