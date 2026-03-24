package tui

import "testing"

func TestRequestCancelRun_ArmsThenCancels(t *testing.T) {
	m := newTestModel()

	cancelCalled := false
	m.runCancel = func() { cancelCalled = true }

	m.requestCancelRun()
	if !m.escPending {
		t.Fatal("expected first cancel request to arm escPending")
	}
	if m.cancelledRun {
		t.Fatal("expected first cancel request to not set cancelledRun")
	}

	m.requestCancelRun()
	if m.escPending {
		t.Fatal("expected second cancel request to clear escPending")
	}
	if !m.cancelledRun {
		t.Fatal("expected second cancel request to set cancelledRun")
	}
	if !cancelCalled {
		t.Fatal("expected run cancel func to be called")
	}
}

func TestBeginShellRun_SetsBusyStateAndExitsShellMode(t *testing.T) {
	m := newTestModel()
	m.setShellMode(true)
	m.input.SetValue("printf hello")

	updated, cmd := m.beginShellRun("printf hello", true)
	after := updated.(*model)

	if cmd == nil {
		t.Fatal("expected shell run command")
	}
	if !after.busy {
		t.Fatal("expected busy=true after beginShellRun")
	}
	if after.runCancel == nil {
		t.Fatal("expected runCancel to be set")
	}
	if after.shellMode {
		t.Fatal("expected explicit shell run to exit shell mode")
	}
	if got := after.input.Value(); got != "" {
		t.Fatalf("expected input cleared, got %q", got)
	}
}

func TestBeginAgentRun_SetsBusyStateAndStartsTurn(t *testing.T) {
	m := newTestModel()
	m.input.SetValue("hello")

	updated, cmd := m.beginAgentRun("hello")
	after := updated.(*model)

	if cmd == nil {
		t.Fatal("expected agent run command")
	}
	if !after.busy {
		t.Fatal("expected busy=true after beginAgentRun")
	}
	if after.runCancel == nil {
		t.Fatal("expected runCancel to be set")
	}
	if turn := after.session.CurrentTurn(); turn == nil {
		t.Fatal("expected current turn to exist")
	}
}
