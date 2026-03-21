package tools

import (
	"bytes"
	"context"
	"errors"
	"fmt"
	"os/exec"
	"strings"

	"github.com/liznear/hh/agent"
)

type GrepResult struct {
	Pattern    string
	TargetPath string
	MatchCount int
	FileCount  int
}

func (r GrepResult) Summary() string {
	if r.MatchCount == 0 {
		return "no matches"
	}
	return fmt.Sprintf("%d matches in %d files", r.MatchCount, r.FileCount)
}

func NewGrepTool() agent.Tool {
	return agent.Tool{
		Name:        "grep",
		Description: "Search file content using ripgrep",
		Schema: map[string]any{
			"type": "object",
			"properties": map[string]any{
				"pattern": map[string]any{"type": "string"},
				"path":    map[string]any{"type": "string"},
			},
			"required": []string{"pattern"},
		},
		Handler: agent.FuncToolHandler(handleGrep),
	}
}

func handleGrep(ctx context.Context, params map[string]any) agent.ToolResult {
	pattern, err := requiredString(params, "pattern")
	if err != nil {
		return toolErr("%s", err.Error())
	}

	target, err := optionalString(params, "path")
	if err != nil {
		return toolErr("%s", err.Error())
	}
	if target == "" {
		target = "."
	}

	cmd := exec.CommandContext(ctx, "rg", "--line-number", "--with-filename", "--no-heading", "--color", "never", pattern, target)

	var stdout bytes.Buffer
	var stderr bytes.Buffer
	cmd.Stdout = &stdout
	cmd.Stderr = &stderr

	if err := cmd.Run(); err != nil {
		var exitErr *exec.ExitError
		if errors.As(err, &exitErr) && exitErr.ExitCode() == 1 {
			return agent.ToolResult{Data: "", Result: GrepResult{Pattern: pattern, TargetPath: target}}
		}
		if stderr.Len() > 0 {
			return toolErr("rg failed: %s", stderr.String())
		}
		return toolErr("rg failed: %v", err)
	}

	output := stdout.String()
	return agent.ToolResult{
		Data: output,
		Result: GrepResult{
			Pattern:    pattern,
			TargetPath: target,
			MatchCount: countNonEmptyLines(output),
			FileCount:  countMatchedFiles(output),
		},
	}
}

func countNonEmptyLines(s string) int {
	count := 0
	for _, line := range strings.Split(s, "\n") {
		if strings.TrimSpace(line) != "" {
			count++
		}
	}
	return count
}

func countMatchedFiles(s string) int {
	files := map[string]struct{}{}
	for _, line := range strings.Split(s, "\n") {
		line = strings.TrimSpace(line)
		if line == "" {
			continue
		}
		idx := strings.Index(line, ":")
		if idx <= 0 {
			continue
		}
		files[line[:idx]] = struct{}{}
	}
	return len(files)
}
