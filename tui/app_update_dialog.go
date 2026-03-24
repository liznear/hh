package tui

import tea "charm.land/bubbletea/v2"

func (m *model) handleDialogKeyPress(msg tea.KeyPressMsg, statusCmd tea.Cmd) (tea.Model, tea.Cmd, bool) {
	if m.questionDialog != nil {
		if m.handleQuestionDialogKey(msg) {
			m.refreshViewport()
			return m, statusCmd, true
		}
	}

	if m.modelPicker != nil {
		if m.handleModelPickerKey(msg) {
			m.showRunResult = false
			m.escPending = false
			return m, statusCmd, true
		}
	}

	return m, nil, false
}
