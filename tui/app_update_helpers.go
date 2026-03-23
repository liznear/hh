package tui

import (
	"time"

	"charm.land/bubbles/v2/stopwatch"
	"github.com/liznear/hh/tui/session"
)

func (m *model) recordScrollInteraction(inputType string, startedAt time.Time, deltaRows int, updateGap time.Duration, timeSinceView time.Duration) {
	m.runtime.autoScroll = m.isListAtBottom(m.messageWidth, m.messageHeight)
	m.runtime.suppressRefreshUntil = time.Now().Add(scrollPriorityWindow)
	if m.runtime.pendingScrollAt.IsZero() {
		m.runtime.pendingScrollAt = time.Now()
		m.runtime.pendingScrollEvents = 0
	}
	m.runtime.pendingScrollEvents++
	if m.runtime.autoScroll && m.runtime.viewportDirty {
		m.refreshViewport()
		m.runtime.viewportDirty = false
	}
	if !m.runtime.debug {
		return
	}
	m.runtime.lastScrollStats = scrollPerfStats{
		inputType:      inputType,
		viewportUpdate: time.Since(startedAt),
		deltaRows:      deltaRows,
		updateGap:      updateGap,
		timeSinceView:  timeSinceView,
	}
	m.runtime.maxScrollStats.viewportUpdate = maxDuration(m.runtime.maxScrollStats.viewportUpdate, m.runtime.lastScrollStats.viewportUpdate)
	m.runtime.maxScrollStats.updateGap = maxDuration(m.runtime.maxScrollStats.updateGap, m.runtime.lastScrollStats.updateGap)
	m.runtime.maxScrollStats.timeSinceView = maxDuration(m.runtime.maxScrollStats.timeSinceView, m.runtime.lastScrollStats.timeSinceView)
}

func (m *model) refreshAfterStreamEvent() {
	if m.runtime.autoScroll || m.isListAtBottom(m.messageWidth, m.messageHeight) {
		if m.shouldRefreshNow() {
			m.refreshViewport()
			m.runtime.viewportDirty = false
			m.runtime.lastRefreshAt = time.Now()
			return
		}
	}
	m.runtime.viewportDirty = true
}

func (m *model) finalizeRun(runErr error) {
	m.runtime.busy = false
	m.runtime.escPending = false
	m.runtime.runCancel = nil
	m.stopwatch, _ = m.stopwatch.Update(stopwatch.StartStopMsg{ID: m.stopwatch.ID()})
	m.runtime.showRunResult = true
	m.runtime.queuedSteering = nil
	if runErr != nil {
		m.addItem(&session.ErrorItem{Message: runErr.Error()})
	}
	if turn := m.session.CurrentTurn(); turn != nil {
		if m.runtime.cancelledRun {
			turn.EndWithStatus("cancelled")
		} else {
			turn.End()
		}
		m.persistTurnEnd(turn)
	}
	m.runtime.cancelledRun = false
	m.stream = nil
	m.refreshViewport()
	m.runtime.lastRefreshAt = time.Now()
	m.runtime.viewportDirty = false
}

func (m *model) hasPendingToolCalls() bool {
	return len(m.toolCalls) > 0
}

func (m *model) shouldRefreshNow() bool {
	if !m.runtime.suppressRefreshUntil.IsZero() && time.Now().Before(m.runtime.suppressRefreshUntil) {
		return false
	}
	if m.runtime.lastRefreshAt.IsZero() {
		return true
	}
	return time.Since(m.runtime.lastRefreshAt) >= renderRefreshInterval
}
