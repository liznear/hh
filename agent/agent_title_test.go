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

func TestAgentRunner_EmitsInitialUserMessageEvent(t *testing.T) {
	provider := &titleMockProvider{responses: []ProviderResponse{{Message: Message{Role: RoleAssistant, Content: "Done"}}}}
	runner := NewAgentRunner("test-model", provider)

	var messageEvents []EventDataMessage
	err := runner.Run(context.Background(), Input{Content: "hello world", Type: "text"}, func(e Event) {
		if e.Type != EventTypeMessage {
			return
		}
		data, ok := e.Data.(EventDataMessage)
		if !ok {
			t.Fatalf("message event data type = %T", e.Data)
		}
		messageEvents = append(messageEvents, data)
	})
	if err != nil {
		t.Fatalf("run failed: %v", err)
	}
	if len(messageEvents) == 0 {
		t.Fatal("expected at least one message event")
	}
	first := messageEvents[0].Message
	if first.Role != RoleUser {
		t.Fatalf("first message role = %q, want %q", first.Role, RoleUser)
	}
	if first.Content != "hello world" {
		t.Fatalf("first message content = %q, want %q", first.Content, "hello world")
	}
}
