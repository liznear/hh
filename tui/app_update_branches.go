package tui

import (
	tea "charm.land/bubbletea/v2"
)

func (m *model) handleWindowSizeMsg(msg tea.WindowSizeMsg, statusCmd tea.Cmd) (tea.Model, tea.Cmd) {
	m.width = msg.Width
	m.height = msg.Height
	m.syncLayout()
	m.refreshViewport()
	return m, statusCmd
}
