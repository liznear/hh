package tools

import (
	"context"
	"fmt"
	"os"
	"strings"

	"github.com/liznear/hh/agent"
	"github.com/pmezard/go-difflib/difflib"
)

type EditResult struct {
	Path         string
	UnifiedDiff  string
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

	unifiedDiff, err := buildUnifiedDiff(path, content, updated)
	if err != nil {
		return toolErr("failed to generate unified diff: %v", err)
	}
	addedLines, deletedLines := countUnifiedDiffChanges(unifiedDiff)

	return agent.ToolResult{
		Data: "ok",
		Result: EditResult{
			Path:         path,
			UnifiedDiff:  unifiedDiff,
			AddedLines:   addedLines,
			DeletedLines: deletedLines,
		},
	}
}

func buildUnifiedDiff(path string, original string, updated string) (string, error) {
	text, err := difflib.GetUnifiedDiffString(difflib.UnifiedDiff{
		A:        difflib.SplitLines(original),
		B:        difflib.SplitLines(updated),
		FromFile: path,
		ToFile:   path,
		Context:  3,
	})
	if err != nil {
		return "", err
	}
	return strings.TrimRight(text, "\n"), nil
}

func countUnifiedDiffChanges(diff string) (added int, deleted int) {
	if strings.TrimSpace(diff) == "" {
		return 0, 0
	}

	for _, line := range strings.Split(diff, "\n") {
		switch {
		case strings.HasPrefix(line, "+++"):
			continue
		case strings.HasPrefix(line, "---"):
			continue
		case strings.HasPrefix(line, "@@"):
			continue
		case strings.HasPrefix(line, "+"):
			added++
		case strings.HasPrefix(line, "-"):
			deleted++
		}
	}

	return added, deleted
}
