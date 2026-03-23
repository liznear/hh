package agent

import (
	"context"
	"errors"
	"testing"
	"time"
)

func TestInteractionManager_RequestAndSubmit(t *testing.T) {
	mgr := NewInteractionManager()
	req := InteractionRequest{
		InteractionID: "interaction_1",
		RunID:         "run_1",
		Kind:          InteractionKindQuestion,
		Title:         "Choose",
		Options: []InteractionOption{
			{ID: "a", Title: "Option A", Description: "A"},
			{ID: "b", Title: "Option B", Description: "B"},
		},
		AllowCustomOption: false,
	}

	resultCh := make(chan InteractionResponse, 1)
	errCh := make(chan error, 1)

	go func() {
		resp, err := mgr.Request(context.Background(), req, nil)
		if err != nil {
			errCh <- err
			return
		}
		resultCh <- resp
	}()

	time.Sleep(10 * time.Millisecond)
	err := mgr.Submit(InteractionResponse{
		InteractionID:    "interaction_1",
		RunID:            "run_1",
		SelectedOptionID: "b",
	})
	if err != nil {
		t.Fatalf("submit failed: %v", err)
	}

	select {
	case callErr := <-errCh:
		t.Fatalf("request failed: %v", callErr)
	case resp := <-resultCh:
		if resp.SelectedOptionID != "b" {
			t.Fatalf("expected selected option b, got %q", resp.SelectedOptionID)
		}
	case <-time.After(time.Second):
		t.Fatal("timed out waiting for interaction response")
	}
}

func TestInteractionManager_SubmitUnknownInteraction(t *testing.T) {
	mgr := NewInteractionManager()
	err := mgr.Submit(InteractionResponse{InteractionID: "missing", SelectedOptionID: "a"})
	if !errors.Is(err, ErrUnknownInteraction) {
		t.Fatalf("expected ErrUnknownInteraction, got %v", err)
	}
}

func TestInteractionManager_DuplicateSubmit(t *testing.T) {
	mgr := NewInteractionManager()
	req := InteractionRequest{
		InteractionID: "interaction_2",
		RunID:         "run_2",
		Kind:          InteractionKindQuestion,
		Title:         "Choose",
		Options:       []InteractionOption{{ID: "a", Title: "Option A", Description: "A"}},
	}

	done := make(chan struct{})
	go func() {
		defer close(done)
		_, _ = mgr.Request(context.Background(), req, nil)
	}()

	time.Sleep(10 * time.Millisecond)
	if err := mgr.Submit(InteractionResponse{InteractionID: "interaction_2", SelectedOptionID: "a"}); err != nil {
		t.Fatalf("first submit failed: %v", err)
	}
	if err := mgr.Submit(InteractionResponse{InteractionID: "interaction_2", SelectedOptionID: "a"}); !errors.Is(err, ErrDuplicateInteractionReply) {
		t.Fatalf("expected duplicate interaction response, got %v", err)
	}
	<-done
}

func TestInteractionManager_Expiration(t *testing.T) {
	mgr := NewInteractionManager()
	expiresAt := time.Now().UTC().Add(20 * time.Millisecond)
	req := InteractionRequest{
		InteractionID: "interaction_3",
		Kind:          InteractionKindQuestion,
		Title:         "Choose",
		Options:       []InteractionOption{{ID: "a", Title: "Option A", Description: "A"}},
		ExpiresAt:     &expiresAt,
	}

	_, err := mgr.Request(context.Background(), req, nil)
	if !errors.Is(err, ErrInteractionExpired) {
		t.Fatalf("expected ErrInteractionExpired, got %v", err)
	}
}

func TestInteractionManager_Dismiss(t *testing.T) {
	mgr := NewInteractionManager()
	req := InteractionRequest{
		InteractionID: "interaction_dismiss",
		RunID:         "run_dismiss",
		Kind:          InteractionKindQuestion,
		Title:         "Dismiss me",
		Options:       []InteractionOption{{ID: "a", Title: "A", Description: "A"}},
	}

	errCh := make(chan error, 1)
	go func() {
		_, err := mgr.Request(context.Background(), req, nil)
		errCh <- err
	}()

	time.Sleep(10 * time.Millisecond)
	if err := mgr.Dismiss("interaction_dismiss"); err != nil {
		t.Fatalf("dismiss failed: %v", err)
	}

	select {
	case err := <-errCh:
		if !errors.Is(err, ErrInteractionDismissed) {
			t.Fatalf("expected ErrInteractionDismissed, got %v", err)
		}
	case <-time.After(time.Second):
		t.Fatal("timed out waiting for dismissed interaction")
	}
}

func TestRunAgentLoop_SyntheticInteractionPauseResume(t *testing.T) {
	provider := &mockProvider{
		responses: []ProviderResponse{
			{ToolCalls: []ToolCall{{ID: "tool_1", Name: "ask", Arguments: `{}`}}},
			{Message: Message{Role: RoleAssistant, Content: "done"}},
		},
	}
	interactionMgr := NewInteractionManager()
	aCtx := Context{
		Model:        "test-model",
		Provider:     provider,
		SystemPrompt: "sys",
		RunID:        "run_synthetic",
		Interactions: interactionMgr,
		Tools: map[string]Tool{
			"ask": {
				Name:        "ask",
				Description: "ask synthetic interaction",
				Handler: FuncToolHandler(func(ctx context.Context, _ map[string]any) ToolResult {
					resp, err := RequestInteraction(ctx, InteractionRequest{
						InteractionID: "interaction_synthetic",
						Kind:          InteractionKindQuestion,
						Title:         "Question",
						Options: []InteractionOption{
							{ID: "allow", Title: "Allow", Description: "Allow action"},
							{ID: "deny", Title: "Deny", Description: "Deny action"},
						},
					})
					if err != nil {
						return ToolResult{IsErr: true, Data: err.Error()}
					}
					return ToolResult{Data: resp.SelectedOptionID}
				}),
			},
		},
	}

	var events []Event
	RunAgentLoop(context.Background(), aCtx, func(e Event) {
		events = append(events, e)
		if e.Type == EventTypeInteractionRequested {
			err := interactionMgr.Submit(InteractionResponse{
				InteractionID:    e.InteractionID,
				RunID:            e.RunID,
				SelectedOptionID: "allow",
			})
			if err != nil {
				t.Fatalf("submit failed: %v", err)
			}
		}
	})

	foundRequested := false
	foundResponded := false
	for _, e := range events {
		switch e.Type {
		case EventTypeInteractionRequested:
			foundRequested = true
		case EventTypeInteractionResponded:
			foundResponded = true
		}
	}
	if !foundRequested {
		t.Fatalf("expected interaction_requested event")
	}
	if !foundResponded {
		t.Fatalf("expected interaction_responded event")
	}

	last := events[len(events)-1]
	if last.Type != EventTypeAgentEnd {
		t.Fatalf("expected final event agent_end, got %s", last.Type)
	}
	data, ok := last.Data.(EventDataAgentEnd)
	if !ok {
		t.Fatalf("expected EventDataAgentEnd, got %T", last.Data)
	}
	hasAllowToolMessage := false
	for _, msg := range data.Messages {
		if msg.Role == RoleTool && msg.Content == "allow" {
			hasAllowToolMessage = true
			break
		}
	}
	if !hasAllowToolMessage {
		t.Fatalf("expected tool message content to include approved option")
	}
}
