package agent

import (
	"context"
	"strings"
	"testing"
)

type testApprover struct{}

func (testApprover) Approve(context.Context, string, map[string]any) error {
	return nil
}

func TestAgentRunner_UpdateAppliesOpts(t *testing.T) {
	runner := NewAgentRunner("test-model", &titleMockProvider{responses: []ProviderResponse{{Message: Message{Role: RoleAssistant, Content: "ok"}}}})

	tools := map[string]Tool{
		"read": {
			Name:        "read",
			Description: "read file",
			Schema:      map[string]any{"type": "object"},
		},
	}
	approver := testApprover{}

	err := runner.Update(
		WithSystemPrompt("new prompt"),
		WithTools(tools),
		WithToolApprover(approver),
		nil,
	)
	if err != nil {
		t.Fatalf("Update() error = %v", err)
	}

	if runner.state.SystemPrompt != "new prompt" {
		t.Fatalf("SystemPrompt = %q, want %q", runner.state.SystemPrompt, "new prompt")
	}
	if len(runner.state.Tools) != 1 {
		t.Fatalf("tools length = %d, want 1", len(runner.state.Tools))
	}
	if _, ok := runner.state.Tools["read"]; !ok {
		t.Fatalf("expected read tool after update")
	}
	if runner.state.Approver == nil {
		t.Fatal("expected approver to be set")
	}
}

func TestAgentRunner_UpdateRejectsActiveRun(t *testing.T) {
	runner := NewAgentRunner("test-model", &titleMockProvider{})
	runner.state.activeRun = &activeRun{runID: "run_1"}

	err := runner.Update(WithSystemPrompt("new prompt"))
	if err == nil {
		t.Fatal("expected error when updating during active run")
	}
	if !strings.Contains(err.Error(), "active run") {
		t.Fatalf("unexpected error: %v", err)
	}
}
