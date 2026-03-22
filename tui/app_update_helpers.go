package tui

import (
	"time"

	"charm.land/bubbles/v2/stopwatch"
	"github.com/liznear/hh/tui/session"
)

func (m *model) recordScrollInteraction(inputType string, startedAt time.Time, deltaRows int, updateGap time.Duration, timeSinceView time.Duration) {
	m.autoScroll = m.isListAtBottom(m.messageWidth, m.messageHeight)
	m.suppressRefreshUntil = time.Now().Add(scrollPriorityWindow)
	if m.pendingScrollAt.IsZero() {
		m.pendingScrollAt = time.Now()
		m.pendingScrollEvents = 0
	}
	m.pendingScrollEvents++
	if m.autoScroll && m.viewportDirty {
		m.refreshViewport()
		m.viewportDirty = false
	}
	if !m.debug {
		return
	}
	m.lastScrollStats = scrollPerfStats{
		inputType:      inputType,
		viewportUpdate: time.Since(startedAt),
		deltaRows:      deltaRows,
		updateGap:      updateGap,
		timeSinceView:  timeSinceView,
	}
	m.maxScrollStats.viewportUpdate = maxDuration(m.maxScrollStats.viewportUpdate, m.lastScrollStats.viewportUpdate)
	m.maxScrollStats.updateGap = maxDuration(m.maxScrollStats.updateGap, m.lastScrollStats.updateGap)
	m.maxScrollStats.timeSinceView = maxDuration(m.maxScrollStats.timeSinceView, m.lastScrollStats.timeSinceView)
}

func (m *model) refreshAfterStreamEvent() {
	if m.autoScroll || m.isListAtBottom(m.messageWidth, m.messageHeight) {
		if m.shouldRefreshNow() {
			m.refreshViewport()
			m.viewportDirty = false
			m.lastRefreshAt = time.Now()
			return
		}
	}
	m.viewportDirty = true
}

func (m *model) finalizeRun(runErr error) {
	m.busy = false
	m.stopwatch, _ = m.stopwatch.Update(stopwatch.StartStopMsg{ID: m.stopwatch.ID()})
	m.showRunResult = true
	if runErr != nil {
		m.addItem(&session.ErrorItem{Message: runErr.Error()})
	}
	if turn := m.session.CurrentTurn(); turn != nil {
		turn.End()
		m.persistTurnEnd(turn)
	}
	m.stream = nil
	m.refreshViewport()
	m.lastRefreshAt = time.Now()
	m.viewportDirty = false
}

func (m *model) hasPendingToolCalls() bool {
	return len(m.toolCalls) > 0
}

func (m *model) shouldRefreshNow() bool {
	if !m.suppressRefreshUntil.IsZero() && time.Now().Before(m.suppressRefreshUntil) {
		return false
	}
	if m.lastRefreshAt.IsZero() {
		return true
	}
	return time.Since(m.lastRefreshAt) >= renderRefreshInterval
}
