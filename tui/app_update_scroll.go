package tui

import (
	"time"

	tea "charm.land/bubbletea/v2"
)

func (m *model) handleMouseWheelMsg(msg tea.MouseWheelMsg, statusCmd tea.Cmd, updateGap time.Duration, timeSinceView time.Duration) (tea.Model, tea.Cmd) {
	scrollUpdateStart := time.Now()
	deltaRows := m.handleMouseWheelScroll(msg)
	if deltaRows != 0 {
		m.recordScrollInteraction("mouse", scrollUpdateStart, deltaRows, updateGap, timeSinceView)
		return m, statusCmd
	}
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
