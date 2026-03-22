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

type BashResult struct {
	Command  string
	ExitCode int
}

func (r BashResult) Summary() string {
	return fmt.Sprintf("exit %d", r.ExitCode)
}

func NewBashTool() agent.Tool {
	return agent.Tool{
		Name:        "bash",
		Description: "Execute a bash command",
		Schema: map[string]any{
			"type": "object",
			"properties": map[string]any{
				"command": map[string]any{"type": "string"},
			},
			"required": []string{"command"},
		},
		Handler: agent.FuncToolHandler(handleBash),
	}
}

func handleBash(ctx context.Context, params map[string]any) agent.ToolResult {
	command, err := requiredString(params, "command")
	if err != nil {
		return toolErr("%s", err.Error())
	}

	cmd := exec.CommandContext(ctx, "bash", "-lc", command)

	var stdout bytes.Buffer
	var stderr bytes.Buffer
	cmd.Stdout = &stdout
	cmd.Stderr = &stderr

	err = cmd.Run()
	exitCode := 0
	if err != nil {
		var exitErr *exec.ExitError
		if errors.As(err, &exitErr) {
			exitCode = exitErr.ExitCode()
		} else {
			return toolErr("bash failed: %v", err)
		}
	}

	output := strings.TrimRight(stdout.String(), "\n")
	stderrOutput := strings.TrimRight(stderr.String(), "\n")
	if stderrOutput != "" {
		if output != "" {
			output += "\n"
		}
		output += stderrOutput
	}

	result := agent.ToolResult{
		Data: output,
		Result: BashResult{
			Command:  command,
			ExitCode: exitCode,
		},
	}
	if err != nil {
		result.IsErr = true
	}

	return result
}
