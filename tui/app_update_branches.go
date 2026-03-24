package tui

import (
	"context"
	"strings"
	"time"

	tea "charm.land/bubbletea/v2"
	"github.com/liznear/hh/tui/session"
)

func (m *model) handleWindowSizeMsg(msg tea.WindowSizeMsg, statusCmd tea.Cmd) (tea.Model, tea.Cmd) {
	m.width = msg.Width
	m.height = msg.Height
	m.syncLayout()
	m.refreshViewport()
	return m, statusCmd
}

func (m *model) handleKeyPressMsg(msg tea.KeyPressMsg, statusCmd tea.Cmd, updateGap time.Duration, timeSinceView time.Duration) (tea.Model, tea.Cmd) {
	if m.questionDialog != nil {
		if m.handleQuestionDialogKey(msg) {
			m.refreshViewport()
			return m, statusCmd
		}
	}

	if m.modelPicker != nil {
		if m.handleModelPickerKey(msg) {
			m.showRunResult = false
			m.escPending = false
			return m, statusCmd
		}
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
		if m.escPending {
			if m.runCancel != nil {
				m.runCancel()
			}
			m.cancelledRun = true
			m.escPending = false
		} else {
			m.escPending = true
		}
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
		return m.startShellRun(command, true)
	}

	if isShellModeInput(inputValue) {
		command := parseShellCommand(inputValue)
		if strings.TrimSpace(command) == "" {
			return m, statusCmd
		}
		return m.startShellRun(command, false)
	}

	if m.handleSlashCommand(prompt) {
		m.input.SetValue("")
		m.showRunResult = false
		m.escPending = false
		return m, statusCmd
	}

	turn := m.session.StartTurn()
	m.persistTurnStart(turn)
	submittedPrompt := promptWithInternalState(prompt, m.session.TodoItems)
	m.input.SetValue("")
	m.busy = true
	m.escPending = false
	m.cancelledRun = false
	m.showRunResult = false
	runCtx, cancel := context.WithCancel(context.Background())
	m.runCancel = cancel
	m.refreshViewport()

	return m, tea.Batch(startAgentStreamCmdWithContext(runCtx, m.runner, submittedPrompt), m.stopwatch.Reset(), m.stopwatch.Start(), func() tea.Msg {
		return m.spinner.Tick()
	})
}

func (m *model) startShellRun(command string, explicitShellMode bool) (tea.Model, tea.Cmd) {
	turn := m.session.StartTurn()
	m.persistTurnStart(turn)
	m.input.SetValue("")
	if explicitShellMode {
		m.setShellMode(false)
	}
	m.busy = true
	m.escPending = false
	m.cancelledRun = false
	m.showRunResult = false
	runCtx, cancel := context.WithCancel(context.Background())
	m.runCancel = cancel
	m.refreshViewport()

	return m, tea.Batch(runShellCommandCmdWithContext(runCtx, command), m.stopwatch.Reset(), m.stopwatch.Start(), func() tea.Msg {
		return m.spinner.Tick()
	})
}

func (m *model) handleMouseWheelMsg(msg tea.MouseWheelMsg, statusCmd tea.Cmd, updateGap time.Duration, timeSinceView time.Duration) (tea.Model, tea.Cmd) {
	scrollUpdateStart := time.Now()
	deltaRows := m.handleMouseWheelScroll(msg)
	if deltaRows != 0 {
		m.recordScrollInteraction("mouse", scrollUpdateStart, deltaRows, updateGap, timeSinceView)
		return m, statusCmd
	}
	return m, statusCmd
}

func (m *model) handleShellCommandDoneMsg(msg shellCommandDoneMsg, statusCmd tea.Cmd) (tea.Model, tea.Cmd) {
	if turn := m.session.CurrentTurn(); turn != nil {
		m.addItemToTurn(turn, &session.ShellMessage{Command: msg.command, Output: msg.output})
	}
	m.finalizeRun(msg.err)
	return m, statusCmd
}

func (m *model) handleSpinnerTickMsg(statusCmd tea.Cmd) (tea.Model, tea.Cmd) {
	if m.busy && m.hasPendingToolCalls() && (m.autoScroll || m.isListAtBottom(m.messageWidth, m.messageHeight)) && m.shouldRefreshNow() {
		m.refreshViewport()
		m.lastRefreshAt = time.Now()
	} else if m.busy && m.hasPendingToolCalls() {
		m.viewportDirty = true
	}
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
