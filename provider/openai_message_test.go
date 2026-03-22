package provider

import (
	"testing"

	"github.com/liznear/hh/agent"
)

func TestToOpenAIMessage_AssistantWithToolCalls(t *testing.T) {
	msg := agent.Message{
		Role:    agent.RoleAssistant,
		Content: "",
		ToolCalls: []agent.ToolCall{
			{ID: "call_1", Name: "bash", Arguments: `{"command":"ls -l"}`},
		},
	}

	got := toOpenAIMessage(&msg)
	if got.OfAssistant == nil {
		t.Fatalf("expected assistant message union variant")
	}

	toolCalls := got.GetToolCalls()
	if len(toolCalls) != 1 {
		t.Fatalf("expected 1 tool call, got %d", len(toolCalls))
	}
	if id := toolCalls[0].GetID(); id == nil || *id != "call_1" {
		t.Fatalf("unexpected tool call id: %v", id)
	}
	if fn := toolCalls[0].GetFunction(); fn == nil || fn.Name != "bash" || fn.Arguments != `{"command":"ls -l"}` {
		t.Fatalf("unexpected function payload: %+v", fn)
	}
}
