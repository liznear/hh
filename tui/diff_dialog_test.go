package tui

import (
	"strings"
	"testing"

	tea "charm.land/bubbletea/v2"
	"github.com/charmbracelet/x/ansi"
)

func TestDiffDialogScroll_WithKeyboardAndWheel(t *testing.T) {
	m := newTestModel()

	oldContent := strings.Repeat("old line\n", 30)
	newContent := strings.Repeat("new line\n", 30)
	m.openDiffDialog("Diff", oldContent, newContent, "sample.go")

	if m.diffDialog == nil {
		t.Fatal("expected diff dialog open")
	}
	if m.diffDialog.ScrollOffset != 0 {
		t.Fatalf("initial scroll offset = %d, want 0", m.diffDialog.ScrollOffset)
	}

	m.handleDiffDialogKey(tea.KeyPressMsg(tea.Key{Code: tea.KeyDown}))
	if m.diffDialog.ScrollOffset != 1 {
		t.Fatalf("scroll offset after down = %d, want 1", m.diffDialog.ScrollOffset)
	}

	m.handleDiffDialogWheel(tea.MouseWheelMsg(tea.Mouse{Button: tea.MouseWheelDown}))
	if m.diffDialog.ScrollOffset <= 1 {
		t.Fatalf("scroll offset after wheel down = %d, want >1", m.diffDialog.ScrollOffset)
	}

	m.handleDiffDialogKey(tea.KeyPressMsg(tea.Key{Code: tea.KeyHome}))
	if m.diffDialog.ScrollOffset != 0 {
		t.Fatalf("scroll offset after home = %d, want 0", m.diffDialog.ScrollOffset)
	}

	view := m.renderDiffDialog(120, 24)
	plain := ansi.Strip(view)
	if !strings.Contains(plain, "Lines ") {
		t.Fatalf("expected scroll position line in dialog, got %q", plain)
	}
}
