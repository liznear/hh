package tui

import (
	"testing"

	tea "charm.land/bubbletea/v2"
	"github.com/liznear/hh/agent"
	"github.com/liznear/hh/tui/session"
)

func TestUpdate_DialogPrecedence_QuestionBeforeModelPicker(t *testing.T) {
	m := newTestModel()
	m.questionDialog = &questionDialogState{
		request: agent.InteractionRequest{
			InteractionID: "interaction_1",
			Kind:          agent.InteractionKindQuestion,
			Title:         "Pick one",
			Options: []agent.InteractionOption{
				{ID: "option_1", Title: "A", Description: "first"},
				{ID: "option_2", Title: "B", Description: "second"},
			},
		},
	}
	m.modelPicker = &modelPickerState{index: 1}

	updated, _ := m.Update(tea.KeyPressMsg(tea.Key{Code: tea.KeyDown}))
	after := updated.(*model)

	if after.questionDialog == nil {
		t.Fatal("expected question dialog to remain open")
	}
	if got := after.questionDialog.selectedIndex; got != 1 {
		t.Fatalf("question selectedIndex = %d, want 1", got)
	}
	if got := after.modelPicker.index; got != 1 {
		t.Fatalf("model picker index changed unexpectedly: got %d, want 1", got)
	}
}

func TestFinalizeRun_ClearsRuntimeAndMarksCancelledTurn(t *testing.T) {
	m := newTestModel()
	turn := m.session.StartTurn()

	m.busy = true
	m.escPending = true
	m.cancelledRun = true
	m.runCancel = func() {}
	m.stream = make(chan tea.Msg)
	m.queuedSteering = []queuedSteeringMessage{{Content: "queued"}}
	m.viewportDirty = true

	m.finalizeRun(nil)

	if m.busy {
		t.Fatal("expected busy to be false after finalizeRun")
	}
	if m.escPending {
		t.Fatal("expected escPending to be false after finalizeRun")
	}
	if m.runCancel != nil {
		t.Fatal("expected runCancel to be cleared after finalizeRun")
	}
	if m.stream != nil {
		t.Fatal("expected stream to be cleared after finalizeRun")
	}
	if len(m.queuedSteering) != 0 {
		t.Fatalf("expected queuedSteering cleared, got %d items", len(m.queuedSteering))
	}
	if !m.showRunResult {
		t.Fatal("expected showRunResult to be true after finalizeRun")
	}
	if m.cancelledRun {
		t.Fatal("expected cancelledRun to be reset after finalizeRun")
	}
	if m.viewportDirty {
		t.Fatal("expected viewportDirty to be false after finalizeRun")
	}

	if turn.EndedAt == nil {
		t.Fatal("expected current turn to be ended")
	}
	end, ok := turn.LastItem().(*session.End)
	if !ok {
		t.Fatalf("expected last turn item to be *session.End, got %T", turn.LastItem())
	}
	if end.Status != "cancelled" {
		t.Fatalf("turn end status = %q, want %q", end.Status, "cancelled")
	}
}

func TestUpdate_ScrollDisablesAutoScrollWhenUserMovesUp(t *testing.T) {
	m := newTestModel()
	m.messageWidth = 40
	m.messageHeight = 5

	for i := 0; i < 20; i++ {
		m.session.AddItem(&session.UserMessage{Content: "line"})
	}
	m.scrollListToBottom(m.messageWidth, m.messageHeight)
	m.autoScroll = true

	updated, _ := m.Update(tea.KeyPressMsg(tea.Key{Code: tea.KeyUp}))
	after := updated.(*model)

	if after.autoScroll {
		t.Fatal("expected autoScroll to be false after manual upward scroll")
	}
	if after.pendingScrollAt.IsZero() {
		t.Fatal("expected pendingScrollAt to be set after scroll interaction")
	}
	if after.pendingScrollEvents < 1 {
		t.Fatalf("expected pendingScrollEvents >= 1, got %d", after.pendingScrollEvents)
	}
}

func TestUpdate_WindowResizeIsStableAcrossRepeatedSameSize(t *testing.T) {
	m := newTestModel()

	updated, _ := m.Update(tea.WindowSizeMsg{Width: 120, Height: 40})
	afterFirst := updated.(*model)

	firstMessageWidth := afterFirst.messageWidth
	firstMessageHeight := afterFirst.messageHeight
	firstInputWidth := afterFirst.input.Width()

	updated, _ = afterFirst.Update(tea.WindowSizeMsg{Width: 120, Height: 40})
	afterSecond := updated.(*model)

	if afterSecond.messageWidth != firstMessageWidth {
		t.Fatalf("messageWidth changed across identical resize: first=%d second=%d", firstMessageWidth, afterSecond.messageWidth)
	}
	if afterSecond.messageHeight != firstMessageHeight {
		t.Fatalf("messageHeight changed across identical resize: first=%d second=%d", firstMessageHeight, afterSecond.messageHeight)
	}
	if afterSecond.input.Width() != firstInputWidth {
		t.Fatalf("input width changed across identical resize: first=%d second=%d", firstInputWidth, afterSecond.input.Width())
	}
	if afterSecond.messageWidth <= 0 || afterSecond.messageHeight <= 0 {
		t.Fatalf("expected positive viewport after resize, got width=%d height=%d", afterSecond.messageWidth, afterSecond.messageHeight)
	}
}

func TestUpdate_BusyEscPendingClearsOnNonEscKey(t *testing.T) {
	m := newTestModel()
	m.busy = true

	updated, _ := m.Update(tea.KeyPressMsg(tea.Key{Code: tea.KeyEscape}))
	afterEsc := updated.(*model)
	if !afterEsc.escPending {
		t.Fatal("expected escPending after first Esc while busy")
	}

	updated, _ = afterEsc.Update(tea.KeyPressMsg(tea.Key{Code: tea.KeySpace, Text: " "}))
	afterRune := updated.(*model)
	if afterRune.escPending {
		t.Fatal("expected escPending to clear after non-Esc key")
	}
}

func TestUpdate_BusyBangDoesNotEnterShellMode(t *testing.T) {
	m := newTestModel()
	m.busy = true

	updated, _ := m.Update(tea.KeyPressMsg(tea.Key{Code: '!', Text: "!"}))
	after := updated.(*model)

	if after.shellMode {
		t.Fatal("expected shellMode to remain false while busy")
	}
	if got := after.input.Value(); got != "!" {
		t.Fatalf("expected busy bang key to be regular input, got %q", got)
	}
}

func TestUpdate_BusyEnterWithoutRunnerAddsErrorAndKeepsInput(t *testing.T) {
	m := newTestModel()
	m.busy = true
	m.runner = nil
	m.input.SetValue("steer this")

	updated, _ := m.Update(tea.KeyPressMsg(tea.Key{Code: tea.KeyEnter}))
	after := updated.(*model)

	last := after.session.LastItem()
	errItem, ok := last.(*session.ErrorItem)
	if !ok {
		t.Fatalf("expected last item to be error, got %T", last)
	}
	if errItem.Message != "runner unavailable" {
		t.Fatalf("unexpected error message: %q", errItem.Message)
	}
	if got := after.input.Value(); got != "steer this" {
		t.Fatalf("expected input to remain unchanged on failed steer submit, got %q", got)
	}
	if len(after.queuedSteering) != 0 {
		t.Fatalf("expected no queued steering item on failed submit, got %d", len(after.queuedSteering))
	}
}
