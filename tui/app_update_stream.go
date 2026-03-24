package tui

import (
	tea "charm.land/bubbletea/v2"
	"github.com/liznear/hh/tui/session"
)

func (m *model) handleShellCommandDoneMsg(msg shellCommandDoneMsg, statusCmd tea.Cmd) (tea.Model, tea.Cmd) {
	if turn := m.session.CurrentTurn(); turn != nil {
		m.addItemToTurn(turn, &session.ShellMessage{Command: msg.command, Output: msg.output})
	}
	m.finalizeRun(msg.err)
	return m, statusCmd
}

func (m *model) handleAgentStreamStartedMsg(msg agentStreamStartedMsg, statusCmd tea.Cmd) (tea.Model, tea.Cmd) {
	m.stream = msg.ch
	return m, tea.Batch(statusCmd, waitForStreamCmd(m.stream))
}

func (m *model) handleStreamBatchMsg(msg streamBatchMsg, statusCmd tea.Cmd) (tea.Model, tea.Cmd) {
	if len(msg.events) > 0 {
		for _, e := range msg.events {
			m.handleAgentEvent(e)
		}
		m.refreshAfterStreamEvent()
	}

	if msg.done {
		m.finalizeRun(msg.doneErr)
		return m, statusCmd
	}

	return m, tea.Batch(statusCmd, waitForStreamCmd(m.stream))
}

func (m *model) handleAgentEventMsg(msg agentEventMsg, statusCmd tea.Cmd) (tea.Model, tea.Cmd) {
	m.handleAgentEvent(msg.event)
	m.refreshAfterStreamEvent()
	return m, tea.Batch(statusCmd, waitForStreamCmd(m.stream))
}

func (m *model) handleAgentRunDoneMsg(msg agentRunDoneMsg, statusCmd tea.Cmd) (tea.Model, tea.Cmd) {
	m.finalizeRun(msg.err)
	return m, statusCmd
}
