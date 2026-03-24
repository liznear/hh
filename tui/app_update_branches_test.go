package tui

import (
	"testing"
	"time"

	tea "charm.land/bubbletea/v2"
	"github.com/liznear/hh/agent"
	"github.com/liznear/hh/tui/session"
)

func TestHandleEnterKey_StartsAgentRunWhenIdle(t *testing.T) {
	m := newTestModel()
	m.input.SetValue("hello")

	updated, cmd := m.handleEnterKey(tea.KeyPressMsg(tea.Key{Code: tea.KeyEnter}), nil)
	after := updated.(*model)

	if cmd == nil {
		t.Fatal("expected non-nil command for agent run start")
	}
	if !after.busy {
		t.Fatal("expected busy=true after starting agent run")
	}
	if after.runCancel == nil {
		t.Fatal("expected runCancel to be set after starting run")
	}
	if got := after.input.Value(); got != "" {
		t.Fatalf("expected input cleared after submit, got %q", got)
	}
	if turn := after.session.CurrentTurn(); turn == nil {
		t.Fatal("expected turn to be started")
	}
}

func TestHandleStreamBatchMsg_DoneFinalizesRun(t *testing.T) {
	m := newTestModel()
	m.session.StartTurn()
	m.busy = true
	m.stream = make(chan tea.Msg)
	m.runCancel = func() {}

	updated, _ := m.handleStreamBatchMsg(streamBatchMsg{done: true}, nil)
	after := updated.(*model)

	if after.busy {
		t.Fatal("expected busy=false after done stream batch")
	}
	if after.stream != nil {
		t.Fatal("expected stream cleared after finalize")
	}
	if after.runCancel != nil {
		t.Fatal("expected runCancel cleared after finalize")
	}
	if !after.showRunResult {
		t.Fatal("expected showRunResult=true after finalize")
	}
	if turn := after.session.CurrentTurn(); turn == nil || turn.EndedAt == nil {
		t.Fatal("expected current turn ended after done stream batch")
	}
}

func TestHandleStreamBatchMsg_WithEventsRefreshesState(t *testing.T) {
	m := newTestModel()
	m.messageWidth = 80
	m.messageHeight = 20
	m.autoScroll = true
	m.lastRefreshAt = time.Time{}

	e := agent.Event{
		Type: agent.EventTypeMessage,
		Data: agent.EventDataMessage{Message: agent.Message{Role: agent.RoleUser, Content: "from stream"}},
	}

	updated, _ := m.handleStreamBatchMsg(streamBatchMsg{events: []agent.Event{e}}, nil)
	after := updated.(*model)

	if after.lastRefreshAt.IsZero() {
		t.Fatal("expected lastRefreshAt updated after stream events")
	}
	if after.viewportDirty {
		t.Fatal("expected viewportDirty=false after immediate stream refresh")
	}
	if last := after.session.LastItem(); last == nil {
		t.Fatal("expected session to contain streamed event output")
	}
}

func TestHandleSpinnerTickMsg_DeferredMarksViewportDirty(t *testing.T) {
	m := newTestModel()
	m.busy = true
	m.autoScroll = true
	m.suppressRefreshUntil = time.Now().Add(time.Second)
	m.toolCalls = map[string]*session.ToolCallItem{"tool-1": {ID: "tool-1"}}

	updated, _ := m.handleSpinnerTickMsg(nil)
	after := updated.(*model)

	if !after.viewportDirty {
		t.Fatal("expected viewportDirty=true when spinner refresh is deferred")
	}
}
