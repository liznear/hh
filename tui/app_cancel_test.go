package tui

import (
	"strings"
	"testing"
	"time"

	tea "charm.land/bubbletea/v2"
)

func TestRenderStatusWidget_ShowsEscInterruptHintWhileBusy(t *testing.T) {
	view := renderStatusWidget(statusWidgetModel{
		Busy:        true,
		SpinnerView: ".",
		Elapsed:     time.Second,
		EscPending:  true,
	}, DefaultTheme())

	if !strings.Contains(view, "esc again to interrupt") {
		t.Fatalf("expected esc hint in status view, got %q", view)
	}
}

func TestRenderStatusWidget_ShellModeShowsShell(t *testing.T) {
	view := renderStatusWidget(statusWidgetModel{ShellMode: true}, DefaultTheme())
	if !strings.Contains(view, "Shell") {
		t.Fatalf("expected shell status label, got %q", view)
	}
	if strings.Contains(view, "Build") {
		t.Fatalf("expected shell status to replace normal status, got %q", view)
	}
}

func TestUpdate_EscTwiceCancelsBusyRun(t *testing.T) {
	m := newTestModel()
	m.busy = true

	cancelCalled := false
	m.runCancel = func() {
		cancelCalled = true
	}

	esc := tea.KeyPressMsg(tea.Key{Code: tea.KeyEscape})

	updated, _ := m.Update(esc)
	m1 := updated.(*model)
	if !m1.escPending {
		t.Fatal("expected first Esc to set escPending")
	}
	if m1.cancelledRun {
		t.Fatal("expected first Esc to not mark run as cancelled")
	}
	if cancelCalled {
		t.Fatal("expected first Esc to not call cancel")
	}

	updated, _ = m1.Update(esc)
	m2 := updated.(*model)
	if m2.escPending {
		t.Fatal("expected second Esc to clear escPending")
	}
	if !m2.cancelledRun {
		t.Fatal("expected second Esc to mark run as cancelled")
	}
	if !cancelCalled {
		t.Fatal("expected second Esc to call cancel")
	}
}
