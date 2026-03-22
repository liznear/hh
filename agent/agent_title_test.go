package agent

import (
	"context"
	"testing"
)

type titleMockProvider struct {
	responses []ProviderResponse
	idx       int
}

func (m *titleMockProvider) ChatCompletionStream(_ context.Context, _ ProviderRequest, onEvent func(ProviderStreamEvent) error) (ProviderResponse, error) {
	_ = onEvent
	if m.idx >= len(m.responses) {
		return ProviderResponse{}, nil
	}
	res := m.responses[m.idx]
	m.idx++
	return res, nil
}

func TestAgentRunner_EmitsSessionTitleOnFirstRun(t *testing.T) {
	provider := &titleMockProvider{responses: []ProviderResponse{
		{Message: Message{Role: RoleAssistant, Content: "Fix flaky retries"}},
		{Message: Message{Role: RoleAssistant, Content: "Done"}},
	}}
	runner := NewAgentRunner("test-model", provider)

	var titleEvents int
	err := runner.Run(context.Background(), Input{Content: "Investigate our flaky retries", Type: "text"}, func(e Event) {
		if e.Type != EventTypeSessionTitle {
			return
		}
		titleEvents++
		data, ok := e.Data.(EventDataSessionTitle)
		if !ok {
			t.Fatalf("title event data type = %T", e.Data)
		}
		if data.Title != "Fix flaky retries" {
			t.Fatalf("title = %q, want %q", data.Title, "Fix flaky retries")
		}
	})
	if err != nil {
		t.Fatalf("run failed: %v", err)
	}
	if titleEvents != 1 {
		t.Fatalf("title events = %d, want 1", titleEvents)
	}
}

func TestNormalizeSessionTitle(t *testing.T) {
	got := normalizeSessionTitle("\n \"A Better Plan\"\nextra")
	if got != "A Better Plan" {
		t.Fatalf("normalizeSessionTitle() = %q, want %q", got, "A Better Plan")
	}
}
