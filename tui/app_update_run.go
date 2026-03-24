package tui

import (
	"context"

	tea "charm.land/bubbletea/v2"
)

func (m *model) beginRun() context.Context {
	m.busy = true
	m.escPending = false
	m.cancelledRun = false
	m.showRunResult = false
	runCtx, cancel := context.WithCancel(context.Background())
	m.runCancel = cancel
	m.refreshViewport()
	return runCtx
}

func (m *model) beginAgentRun(prompt string) (tea.Model, tea.Cmd) {
	turn := m.session.StartTurn()
	m.persistTurnStart(turn)
	submittedPrompt := promptWithInternalState(prompt, m.session.TodoItems)
	m.input.SetValue("")
	runCtx := m.beginRun()

	return m, tea.Batch(startAgentStreamCmdWithContext(runCtx, m.runner, submittedPrompt), m.stopwatch.Reset(), m.stopwatch.Start(), func() tea.Msg {
		return m.spinner.Tick()
	})
}

func (m *model) beginShellRun(command string, explicitShellMode bool) (tea.Model, tea.Cmd) {
	turn := m.session.StartTurn()
	m.persistTurnStart(turn)
	m.input.SetValue("")
	if explicitShellMode {
		m.setShellMode(false)
	}
	runCtx := m.beginRun()

	return m, tea.Batch(runShellCommandCmdWithContext(runCtx, command), m.stopwatch.Reset(), m.stopwatch.Start(), func() tea.Msg {
		return m.spinner.Tick()
	})
}

func (m *model) requestCancelRun() {
	if m.escPending {
		if m.runCancel != nil {
			m.runCancel()
		}
		m.cancelledRun = true
		m.escPending = false
		return
	}
	m.escPending = true
}
