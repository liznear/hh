package agent

import (
	"context"
	"encoding/json"
	"errors"
	"os"
	"path/filepath"
	"reflect"
	"testing"
)

type mockProvider struct {
	responses    []ProviderResponse
	streamEvents [][]ProviderStreamEvent
	err          error
	idx          int
}

func (m *mockProvider) ChatCompletionStream(ctx context.Context, req ProviderRequest, onEvent func(ProviderStreamEvent) error) (ProviderResponse, error) {
	if m.err != nil {
		return ProviderResponse{}, m.err
	}

	if m.idx < len(m.streamEvents) {
		for _, se := range m.streamEvents[m.idx] {
			if err := onEvent(se); err != nil {
				return ProviderResponse{}, err
			}
		}
	}

	if m.idx >= len(m.responses) {
		return ProviderResponse{}, nil
	}
	res := m.responses[m.idx]
	m.idx++
	return res, nil
}

type ProviderStreamSetup struct {
	Responses    []ProviderResponse    `json:"responses"`
	StreamEvents []ProviderStreamEvent `json:"streamEvents"`
}

func TestRunAgentLoop(t *testing.T) {
	sessionsDir := filepath.Join("testdata", "sessions")
	entries, err := os.ReadDir(sessionsDir)
	if err != nil {
		t.Fatalf("failed to read sessions dir: %v", err)
	}
	tools := map[string]Tool{
		"echo": {
			Name:        "echo",
			Description: "Echo back the arguments",
			Handler: FuncToolHandler(func(ctx context.Context, args map[string]any) ToolResult {
				v, _ := json.Marshal(args)
				return ToolResult{Data: string(v)}
			}),
		},
	}

	for _, entry := range entries {
		if !entry.IsDir() {
			continue
		}
		sessionName := entry.Name()
		t.Run(sessionName, func(t *testing.T) {
			sessionDir := filepath.Join(sessionsDir, sessionName)

			// Load Provider Setup
			providerBytes, err := os.ReadFile(filepath.Join(sessionDir, "provider_stream.json"))
			if err != nil {
				t.Fatalf("failed to read provider_stream.json: %v", err)
			}
			var setups []ProviderStreamSetup
			if err := json.Unmarshal(providerBytes, &setups); err != nil {
				t.Fatalf("failed to unmarshal provider_stream.json: %v", err)
			}

			var responses []ProviderResponse
			var streamEvents [][]ProviderStreamEvent
			for _, setup := range setups {
				if len(setup.Responses) > 0 {
					responses = append(responses, setup.Responses[0])
				}
				streamEvents = append(streamEvents, setup.StreamEvents)
			}

			// Load Context
			ctxBytes, err := os.ReadFile(filepath.Join(sessionDir, "context.json"))
			if err != nil {
				t.Fatalf("failed to read context.json: %v", err)
			}
			var aCtx Context
			if err := json.Unmarshal(ctxBytes, &aCtx); err != nil {
				t.Fatalf("failed to unmarshal context.json: %v", err)
			}
			aCtx.Tools = tools
			aCtx.Provider = &mockProvider{
				responses:    responses,
				streamEvents: streamEvents,
			}

			var events []Event
			onEvent := func(e Event) {
				events = append(events, e)
			}

			// Load expected events
			expectedBytes, err := os.ReadFile(filepath.Join(sessionDir, "expected_events.jsonl"))
			if err != nil {
				t.Fatalf("failed to read expected_events.jsonl: %v", err)
			}

			var expectedEvents []Event
			for _, line := range bytesToLines(expectedBytes) {
				var ev Event
				if err := json.Unmarshal([]byte(line), &ev); err != nil {
					t.Fatalf("failed to unmarshal expected event: %v", err)
				}
				expectedEvents = append(expectedEvents, ev)
			}

			ctx := context.Background()
			RunAgentLoop(ctx, aCtx, onEvent)

			if len(events) != len(expectedEvents) {
				t.Fatalf("expected %d events, got %d. events: %v", len(expectedEvents), len(events), events)
			}

			for i, expectedEv := range expectedEvents {
				if events[i].Type != expectedEv.Type {
					t.Errorf("event %d: expected type %v, got %v", i, expectedEv.Type, events[i].Type)
				}

				// Compare Data loosely by converting to JSON to handle type matching
				gotDataBytes, _ := json.Marshal(events[i].Data)
				wantDataBytes, _ := json.Marshal(expectedEv.Data)

				var gotObj, wantObj any
				json.Unmarshal(gotDataBytes, &gotObj)
				json.Unmarshal(wantDataBytes, &wantObj)

				if !reflect.DeepEqual(gotObj, wantObj) {
					t.Errorf("event %d data mismatch.\nGot:  %s\nWant: %s", i, string(gotDataBytes), string(wantDataBytes))
				}
			}
		})
	}
}

func bytesToLines(b []byte) []string {
	var lines []string
	str := string(b)
	lastIdx := 0
	for i := 0; i < len(str); i++ {
		if str[i] == '\n' {
			if i > lastIdx {
				lines = append(lines, str[lastIdx:i])
			}
			lastIdx = i + 1
		}
	}
	if lastIdx < len(str) {
		lines = append(lines, str[lastIdx:])
	}
	return lines
}

func TestRunAgentLoop_ProviderError(t *testing.T) {
	expectedErr := errors.New("provider error")
	mockP := &mockProvider{
		err: expectedErr,
	}

	aCtx := Context{
		Model:        "test-model",
		Provider:     mockP,
		SystemPrompt: "test system prompt",
		Tools:        map[string]Tool{},
	}

	var events []Event
	onEvent := func(e Event) {
		events = append(events, e)
	}

	RunAgentLoop(context.Background(), aCtx, onEvent)

	expectedEventTypes := []EventType{
		EventTypeAgentStart,
		EventTypeTurnStart,
		EventTypeError,
		EventTypeTurnEnd,
		EventTypeAgentEnd,
	}

	if len(events) != len(expectedEventTypes) {
		t.Fatalf("expected %d events, got %d. events: %v", len(expectedEventTypes), len(events), events)
	}

	for i, expectedType := range expectedEventTypes {
		if events[i].Type != expectedType {
			t.Errorf("event %d: expected type %v, got %v", i, expectedType, events[i].Type)
		}
	}

	errData, ok := events[2].Data.(error)
	if !ok {
		t.Fatalf("expected error data, got %T", events[2].Data)
	}
	if errData != expectedErr {
		t.Errorf("expected error %v, got %v", expectedErr, errData)
	}
}

func TestRunAgentLoop_ContextCanceled(t *testing.T) {
	mockP := &mockProvider{
		responses: []ProviderResponse{
			{},
		},
	}

	aCtx := Context{
		Provider: mockP,
	}

	var events []Event
	onEvent := func(e Event) {
		events = append(events, e)
	}

	ctx, cancel := context.WithCancel(context.Background())
	cancel() // Cancel the context before running

	RunAgentLoop(ctx, aCtx, onEvent)

	expectedEventTypes := []EventType{
		EventTypeAgentStart,
		EventTypeError,
		EventTypeAgentEnd,
	}

	if len(events) != len(expectedEventTypes) {
		t.Fatalf("expected %d events, got %d. events: %v", len(expectedEventTypes), len(events), events)
	}

	for i, expectedType := range expectedEventTypes {
		if events[i].Type != expectedType {
			t.Errorf("event %d: expected type %v, got %v", i, expectedType, events[i].Type)
		}
	}

	errData, ok := events[1].Data.(error)
	if !ok {
		t.Fatalf("expected error data, got %T", events[1].Data)
	}
	if errData != context.Canceled {
		t.Errorf("expected error %v, got %v", context.Canceled, errData)
	}
}

func TestRunAgentLoop_PreservesAssistantToolCallsInHistory(t *testing.T) {
	mockP := &mockProvider{
		responses: []ProviderResponse{
			{
				Message:   Message{Role: RoleAssistant, ToolCalls: []ToolCall{{ID: "call_1", Name: "echo", Arguments: `{"value":"ok"}`}}},
				ToolCalls: []ToolCall{{ID: "call_1", Name: "echo", Arguments: `{"value":"ok"}`}},
			},
			{
				Message: Message{Role: RoleAssistant, Content: "done"},
			},
		},
	}

	aCtx := Context{
		Model:        "test-model",
		Provider:     mockP,
		SystemPrompt: "test system prompt",
		Tools: map[string]Tool{
			"echo": {
				Name:        "echo",
				Description: "Echo back the arguments",
				Handler: FuncToolHandler(func(ctx context.Context, args map[string]any) ToolResult {
					return ToolResult{Data: "ok"}
				}),
			},
		},
	}

	var events []Event
	RunAgentLoop(context.Background(), aCtx, func(e Event) {
		events = append(events, e)
	})

	if len(events) == 0 {
		t.Fatalf("expected events")
	}
	last := events[len(events)-1]
	if last.Type != EventTypeAgentEnd {
		t.Fatalf("expected last event %q, got %q", EventTypeAgentEnd, last.Type)
	}

	endData, ok := last.Data.(EventDataAgentEnd)
	if !ok {
		t.Fatalf("expected EventDataAgentEnd, got %T", last.Data)
	}

	if len(endData.Messages) < 3 {
		t.Fatalf("expected at least 3 messages, got %d", len(endData.Messages))
	}

	assistantWithToolCall := endData.Messages[0]
	if assistantWithToolCall.Role != RoleAssistant {
		t.Fatalf("expected first message role assistant, got %q", assistantWithToolCall.Role)
	}
	if len(assistantWithToolCall.ToolCalls) != 1 || assistantWithToolCall.ToolCalls[0].ID != "call_1" {
		t.Fatalf("expected assistant tool call to be preserved, got %+v", assistantWithToolCall.ToolCalls)
	}

	toolMessage := endData.Messages[1]
	if toolMessage.Role != RoleTool || toolMessage.CallID != "call_1" {
		t.Fatalf("expected tool message with call id call_1, got %+v", toolMessage)
	}
}
