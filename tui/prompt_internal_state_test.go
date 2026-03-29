package tui

import (
	"strings"
	"testing"

	"github.com/liznear/hh/tui/session"
)

func TestBuildInternalState_NoTodos(t *testing.T) {
	got := buildInternalState(nil)
	for _, want := range []string{
		"<internal-state>",
		"<timestamp>",
		"</internal-state>",
	} {
		if !strings.Contains(got, want) {
			t.Fatalf("expected internal state to contain %q, got %q", want, got)
		}
	}
	if strings.Contains(got, "<todo-items>") {
		t.Fatalf("expected no todo-items block, got %q", got)
	}
}

func TestBuildInternalState_WithTodos(t *testing.T) {
	todos := []session.TodoItem{
		{Content: "Write tests", Status: session.TodoStatusPending},
		{Content: "Fix <bug>", Status: session.TodoStatusWIP},
	}

	got := buildInternalState(todos)

	for _, want := range []string{
		"<internal-state>",
		"<timestamp>",
		"<todo-items>",
		"<todo-item>",
		"<content>Write tests</content>",
		"<status>pending</status>",
		"<content>Fix &lt;bug&gt;</content>",
		"<status>wip</status>",
		"</internal-state>",
	} {
		if !strings.Contains(got, want) {
			t.Fatalf("expected internal state to contain %q, got %q", want, got)
		}
	}
}
