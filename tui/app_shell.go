package tui

import (
	"bytes"
	"context"
	"errors"
	"os/exec"
	"strings"

	tea "charm.land/bubbletea/v2"
)

type shellCommandDoneMsg struct {
	command string
	output  string
	err     error
}

func isShellModeInput(input string) bool {
	trimmed := strings.TrimLeft(input, " \t")
	return strings.HasPrefix(trimmed, "!")
}

func parseShellCommand(input string) string {
	trimmed := strings.TrimLeft(input, " \t")
	if !strings.HasPrefix(trimmed, "!") {
		return ""
	}
	return strings.TrimSpace(strings.TrimPrefix(trimmed, "!"))
}

func (m *model) setShellMode(enabled bool) {
	m.runtime.shellMode = enabled
	if enabled {
		applyTextareaPromptColor(&m.input, m.theme.Color(ThemeColorInputPromptShell))
		return
	}
	applyTextareaPromptColor(&m.input, m.theme.Color(ThemeColorInputPromptDefault))
}

func (m *model) shellModeActive() bool {
	return m.runtime.shellMode || isShellModeInput(m.input.Value())
}

func runShellCommandCmdWithContext(ctx context.Context, command string) tea.Cmd {
	return func() tea.Msg {
		cmd := exec.CommandContext(ctx, "bash", "-lc", command)

		var stdout bytes.Buffer
		var stderr bytes.Buffer
		cmd.Stdout = &stdout
		cmd.Stderr = &stderr

		err := cmd.Run()
		output := strings.TrimRight(stdout.String(), "\n")
		stderrOutput := strings.TrimRight(stderr.String(), "\n")
		if stderrOutput != "" {
			if output != "" {
				output += "\n"
			}
			output += stderrOutput
		}

		if err != nil {
			var exitErr *exec.ExitError
			if !errors.As(err, &exitErr) {
				return shellCommandDoneMsg{command: command, output: output, err: err}
			}
		}

		return shellCommandDoneMsg{command: command, output: output}
	}
}
