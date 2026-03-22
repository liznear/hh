package tui

import (
	"strings"
	"testing"

	"github.com/liznear/hh/tui/session"
)

func TestPromptWithInternalState_NoTodos(t *testing.T) {
	prompt := "Actual user input"
	got := promptWithInternalState(prompt, nil)
	if got != prompt {
		t.Fatalf("prompt mismatch: got %q, want %q", got, prompt)
	}
}

func TestPromptWithInternalState_WithTodos(t *testing.T) {
	prompt := "Actual user input"
	todos := []session.TodoItem{
		{Content: "Write tests", Status: session.TodoStatusPending},
		{Content: "Fix <bug>", Status: session.TodoStatusWIP},
	}

	got := promptWithInternalState(prompt, todos)

	for _, want := range []string{
		"<internal-state>",
		"<todo-items>",
		"<todo-item>",
		"<content>Write tests</content>",
		"<status>pending</status>",
		"<content>Fix &lt;bug&gt;</content>",
		"<status>wip</status>",
		"</internal-state>",
	} {
		if !strings.Contains(got, want) {
			t.Fatalf("expected prompt to contain %q, got %q", want, got)
		}
	}
}
