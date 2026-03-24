package tui

import (
	"strings"
	"time"

	tea "charm.land/bubbletea/v2"
	"github.com/liznear/hh/tui/session"
)

func (m *model) handleKeyPressMsg(msg tea.KeyPressMsg, statusCmd tea.Cmd, updateGap time.Duration, timeSinceView time.Duration) (tea.Model, tea.Cmd) {
	if updated, cmd, handled := m.handleDialogKeyPress(msg, statusCmd); handled {
		return updated, cmd
	}

	prevOffset := m.currentListOffset(m.messageWidth)
	scrollUpdateStart := time.Now()
	scrolled, deltaRows := m.handleScrollKey(msg)
	if scrolled {
		if deltaRows == 0 {
			deltaRows = m.currentListOffset(m.messageWidth) - prevOffset
		}
		m.recordScrollInteraction("keyboard", scrollUpdateStart, deltaRows, updateGap, timeSinceView)
		return m, statusCmd
	}

	key := msg.Key()
	if key.Code == tea.KeyEscape && m.busy {
		m.requestCancelRun()
		return m, statusCmd
	}

	if !m.busy {
		if !m.shellMode && msg.String() == "!" && strings.TrimSpace(m.input.Value()) == "" {
			m.setShellMode(true)
			return m, statusCmd
		}

		if m.shellMode && key.Code == tea.KeyBackspace && m.input.Value() == "" {
			m.setShellMode(false)
			return m, statusCmd
		}
	}

	if isInsertNewlineKey(msg) {
		m.input.InsertRune('\n')
		return m, statusCmd
	}

	if key.Code == tea.KeyEnter {
		return m.handleEnterKey(msg, statusCmd)
	}

	switch msg.String() {
	case "ctrl+c":
		return m, tea.Quit
	}
	m.escPending = false

	var cmd tea.Cmd
	m.input, cmd = m.input.Update(msg)
	return m, tea.Batch(statusCmd, cmd)
}

func (m *model) handleEnterKey(_ tea.KeyPressMsg, statusCmd tea.Cmd) (tea.Model, tea.Cmd) {
	inputValue := m.input.Value()
	prompt := strings.TrimSpace(inputValue)
	if prompt == "" {
		return m, statusCmd
	}

	if m.busy {
		if m.runner == nil {
			m.addItem(&session.ErrorItem{Message: "runner unavailable"})
			return m, statusCmd
		}
		if err := m.runner.SubmitSteeringMessage(prompt, ""); err != nil {
			m.addItem(&session.ErrorItem{Message: err.Error()})
			return m, statusCmd
		}
		m.queuedSteering = append(m.queuedSteering, queuedSteeringMessage{Content: prompt})
		m.input.SetValue("")
		m.refreshViewport()
		return m, statusCmd
	}

	if m.shellMode {
		command := strings.TrimSpace(inputValue)
		if command == "" {
			return m, statusCmd
		}
		return m.beginShellRun(command, true)
	}

	if isShellModeInput(inputValue) {
		command := parseShellCommand(inputValue)
		if strings.TrimSpace(command) == "" {
			return m, statusCmd
		}
		return m.beginShellRun(command, false)
	}

	if m.handleSlashCommand(prompt) {
		m.input.SetValue("")
		m.showRunResult = false
		m.escPending = false
		return m, statusCmd
	}

	return m.beginAgentRun(prompt)
}
