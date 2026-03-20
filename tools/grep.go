package tools

import (
	"bytes"
	"context"
	"errors"
	"os/exec"

	"github.com/liznear/hh/agent"
)

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
			return agent.ToolResult{Data: ""}
		}
		if stderr.Len() > 0 {
			return toolErr("rg failed: %s", stderr.String())
		}
		return toolErr("rg failed: %v", err)
	}

	return agent.ToolResult{Data: stdout.String()}
}
