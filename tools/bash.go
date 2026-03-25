package tools

import (
	"bytes"
	"context"
	"errors"
	"fmt"
	"os/exec"
	"strings"
	"syscall"

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
	// Create a new process group so we can kill all child processes on cancellation
	cmd.SysProcAttr = &syscall.SysProcAttr{
		Setpgid: true,
	}

	var stdout bytes.Buffer
	var stderr bytes.Buffer
	cmd.Stdout = &stdout
	cmd.Stderr = &stderr

	err = cmd.Start()
	if err != nil {
		return toolErr("bash failed to start: %v", err)
	}

	// Wait for the process in a goroutine so we can handle cancellation
	done := make(chan error, 1)
	go func() {
		done <- cmd.Wait()
	}()

	select {
	case <-ctx.Done():
		// Context was cancelled - kill the entire process group
		if cmd.Process != nil {
			// Kill the process group (negative PID means kill the group)
			syscall.Kill(-cmd.Process.Pid, syscall.SIGKILL)
		}
		<-done // Wait for the process to actually finish
		return toolErr("bash command interrupted")
	case err := <-done:
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
}
