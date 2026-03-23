package tools

import (
	"context"
	"encoding/json"
	"strings"
	"testing"

	"github.com/liznear/hh/agent"
)

type questionMockProvider struct {
	responses []agent.ProviderResponse
	idx       int
}

func (m *questionMockProvider) ChatCompletionStream(_ context.Context, _ agent.ProviderRequest, _ func(agent.ProviderStreamEvent) error) (agent.ProviderResponse, error) {
	if m.idx >= len(m.responses) {
		return agent.ProviderResponse{}, nil
	}
	resp := m.responses[m.idx]
	m.idx++
	return resp, nil
}

func TestParseQuestionInputValidation(t *testing.T) {
	_, err := parseQuestionInput(map[string]any{})
	if err == nil || !strings.Contains(err.Error(), "question.title") {
		t.Fatalf("expected question.title validation error, got %v", err)
	}

	_, err = parseQuestionInput(map[string]any{
		"question": map[string]any{"title": "Pick"},
		"options":  []any{},
	})
	if err == nil || !strings.Contains(err.Error(), "at least one") {
		t.Fatalf("expected options validation error, got %v", err)
	}
}

func TestMapQuestionResult(t *testing.T) {
	input := questionInput{AllowCustomOption: true}
	req := agent.InteractionRequest{Options: []agent.InteractionOption{{ID: "option_1", Title: "A", Description: "aa"}}}

	optionResult, err := mapQuestionResult(input, req, agent.InteractionResponse{SelectedOptionID: "option_1"})
	if err != nil {
		t.Fatalf("expected option result, got error %v", err)
	}
	if optionResult.Type != "option" || optionResult.Option == nil || optionResult.Option.Index != 1 {
		t.Fatalf("unexpected option result: %+v", optionResult)
	}

	customResult, err := mapQuestionResult(input, req, agent.InteractionResponse{CustomText: " custom answer "})
	if err != nil {
		t.Fatalf("expected custom result, got error %v", err)
	}
	if customResult.Type != "custom" || customResult.Custom != "custom answer" {
		t.Fatalf("unexpected custom result: %+v", customResult)
	}
}

func TestHandleQuestionWithoutInteractionRuntime(t *testing.T) {
	result := handleQuestion(context.Background(), map[string]any{
		"question": map[string]any{"title": "Pick one"},
		"options": []any{
			map[string]any{"title": "A", "description": "desc"},
		},
		"allow_custom_option": false,
	})
	if !result.IsErr {
		t.Fatal("expected error when no interaction runtime is present")
	}
	if !strings.Contains(result.Data, "no active interaction session") {
		t.Fatalf("unexpected error: %s", result.Data)
	}
}

func TestQuestionToolIntegration(t *testing.T) {
	tool := NewQuestionTool()
	provider := &questionMockProvider{responses: []agent.ProviderResponse{
		{
			ToolCalls: []agent.ToolCall{{
				ID:        "call_question",
				Name:      "question",
				Arguments: `{"question":{"title":"Choose"},"options":[{"title":"Allow","description":"Proceed"},{"title":"Deny","description":"Stop"}],"allow_custom_option":false}`,
			}},
		},
		{Message: agent.Message{Role: agent.RoleAssistant, Content: "done"}},
	}}

	interactionMgr := agent.NewInteractionManager()
	aCtx := agent.Context{
		Model:        "test",
		Provider:     provider,
		SystemPrompt: "sys",
		RunID:        "run_q",
		Interactions: interactionMgr,
		Tools: map[string]agent.Tool{
			"question": tool,
		},
	}

	var end agent.EventDataAgentEnd
	agent.RunAgentLoop(context.Background(), aCtx, func(e agent.Event) {
		if e.Type == agent.EventTypeInteractionRequested {
			reqData := e.Data.(agent.EventDataInteractionRequested)
			if err := interactionMgr.Submit(agent.InteractionResponse{
				InteractionID:    reqData.Request.InteractionID,
				RunID:            reqData.Request.RunID,
				SelectedOptionID: "option_1",
			}); err != nil {
				t.Fatalf("submit interaction failed: %v", err)
			}
		}
		if e.Type == agent.EventTypeAgentEnd {
			end = e.Data.(agent.EventDataAgentEnd)
		}
	})

	found := false
	for _, msg := range end.Messages {
		if msg.Role != agent.RoleTool || msg.CallID != "call_question" {
			continue
		}
		var result QuestionResult
		if err := json.Unmarshal([]byte(msg.Content), &result); err != nil {
			t.Fatalf("invalid question tool payload: %v", err)
		}
		if result.Type != "option" || result.Option == nil || result.Option.Title != "Allow" {
			t.Fatalf("unexpected question tool result: %+v", result)
		}
		found = true
	}
	if !found {
		t.Fatal("expected tool message for question call")
	}
}
