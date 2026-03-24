package tui

import (
	"testing"

	tea "charm.land/bubbletea/v2"
	"github.com/liznear/hh/agent"
)

func TestHandleKeyPressMsg_QuestionDialogConsumesInput(t *testing.T) {
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

	updated, _ := m.handleKeyPressMsg(tea.KeyPressMsg(tea.Key{Code: tea.KeyDown}), nil, 0, 0)
	after := updated.(*model)

	if after.questionDialog == nil {
		t.Fatal("expected question dialog to remain open")
	}
	if got := after.questionDialog.selectedIndex; got != 1 {
		t.Fatalf("selectedIndex = %d, want 1", got)
	}
}

func TestHandleKeyPressMsg_ModelPickerEscClosesAndResetsFlags(t *testing.T) {
	m := newTestModel()
	m.modelPicker = &modelPickerState{index: 0}
	m.showRunResult = true
	m.escPending = true

	updated, _ := m.handleKeyPressMsg(tea.KeyPressMsg(tea.Key{Code: tea.KeyEscape}), nil, 0, 0)
	after := updated.(*model)

	if after.modelPicker != nil {
		t.Fatal("expected model picker to close on esc")
	}
	if after.showRunResult {
		t.Fatal("expected showRunResult to reset after picker interaction")
	}
	if after.escPending {
		t.Fatal("expected escPending to reset after picker interaction")
	}
}

func TestHandleKeyPressMsg_ShiftEnterInsertsNewline(t *testing.T) {
	m := newTestModel()
	m.input.SetValue("hello")

	updated, _ := m.handleKeyPressMsg(tea.KeyPressMsg(tea.Key{Code: tea.KeyEnter, Mod: tea.ModShift}), nil, 0, 0)
	after := updated.(*model)

	if got := after.input.Value(); got != "hello\n" {
		t.Fatalf("input = %q, want %q", got, "hello\\n")
	}
}
