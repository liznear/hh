package tools

import (
	"context"
	"fmt"
	"strings"
	"sync"
	"testing"

	"github.com/liznear/hh/agent"
)

type taskIntegrationProvider struct {
	mu             sync.Mutex
	parentToolCall agent.ToolCall
	parentCalls    int
	failSubAgent   error
}

func (p *taskIntegrationProvider) ChatCompletionStream(_ context.Context, req agent.ProviderRequest, _ func(agent.ProviderStreamEvent) error) (agent.ProviderResponse, error) {
	p.mu.Lock()
	defer p.mu.Unlock()

	if hasTool(req.Tools, "task") {
		if p.parentCalls == 0 {
			p.parentCalls++
			return agent.ProviderResponse{ToolCalls: []agent.ToolCall{p.parentToolCall}}, nil
		}
		p.parentCalls++
		return agent.ProviderResponse{Message: agent.Message{Role: agent.RoleAssistant, Content: "done"}}, nil
	}

	if p.failSubAgent != nil {
		return agent.ProviderResponse{}, p.failSubAgent
	}
	prompt := lastUserMessage(req.Messages)
	return agent.ProviderResponse{Message: agent.Message{Role: agent.RoleAssistant, Content: "sub:" + prompt}}, nil
}

func hasTool(tools []agent.Tool, name string) bool {
	for _, tool := range tools {
		if tool.Name == name {
			return true
		}
	}
	return false
}

func lastUserMessage(messages []agent.Message) string {
	for i := len(messages) - 1; i >= 0; i-- {
		if messages[i].Role == agent.RoleUser {
			return strings.TrimSpace(messages[i].Content)
		}
	}
	return ""
}

func TestParseTaskRequests_Single(t *testing.T) {
	requests, err := parseTaskRequests(map[string]any{"tasks": []any{map[string]any{"sub_agent_name": "Explorer", "task": "find x"}}})
	if err != nil {
		t.Fatalf("unexpected error: %v", err)
	}
	if len(requests) != 1 {
		t.Fatalf("len = %d, want 1", len(requests))
	}
	if requests[0].SubAgentName != "Explorer" || requests[0].Task != "find x" {
		t.Fatalf("unexpected request: %+v", requests[0])
	}
}

func TestParseTaskRequests_Multiple(t *testing.T) {
	requests, err := parseTaskRequests(map[string]any{
		"tasks": []any{
			map[string]any{"sub_agent_name": "Explorer", "task": "one"},
			map[string]any{"sub_agent_name": "Explorer", "task": "two"},
		},
	})
	if err != nil {
		t.Fatalf("unexpected error: %v", err)
	}
	if len(requests) != 2 {
		t.Fatalf("len = %d, want 2", len(requests))
	}
}

func TestTaskToolSchema_ContainsSubAgentEnum(t *testing.T) {
	tool := NewTaskTool()
	properties, ok := tool.Schema["properties"].(map[string]any)
	if !ok {
		t.Fatalf("schema properties missing or invalid")
	}
	tasks, ok := properties["tasks"].(map[string]any)
	if !ok {
		t.Fatalf("tasks schema missing")
	}
	items, ok := tasks["items"].(map[string]any)
	if !ok {
		t.Fatalf("tasks.items schema missing")
	}
	itemProperties, ok := items["properties"].(map[string]any)
	if !ok {
		t.Fatalf("tasks.items.properties missing")
	}
	subAgent, ok := itemProperties["sub_agent_name"].(map[string]any)
	if !ok {
		t.Fatalf("sub_agent_name schema missing")
	}
	enum, ok := subAgent["enum"].([]string)
	if !ok {
		enumAny, ok := subAgent["enum"].([]any)
		if !ok {
			t.Fatalf("sub_agent_name enum missing")
		}
		enum = make([]string, 0, len(enumAny))
		for _, v := range enumAny {
			s, ok := v.(string)
			if !ok {
				continue
			}
			enum = append(enum, s)
		}
	}
	if len(enum) == 0 {
		t.Fatalf("expected non-empty sub-agent enum")
	}
	foundExplorer := false
	for _, name := range enum {
		if name == "Explorer" {
			foundExplorer = true
			break
		}
	}
	if !foundExplorer {
		t.Fatalf("expected Explorer in sub-agent enum, got %v", enum)
	}
}

func TestTaskToolIntegration_Single(t *testing.T) {
	provider := &taskIntegrationProvider{parentToolCall: agent.ToolCall{
		ID:        "call_task",
		Name:      "task",
		Arguments: `{"tasks":[{"sub_agent_name":"Explorer","task":"inspect repo"}]}`,
	}}

	end := runTaskToolIntegration(t, provider)
	toolOutput := findToolOutput(end.Messages, "call_task")
	if toolOutput != "sub:inspect repo" {
		t.Fatalf("tool output = %q, want %q", toolOutput, "sub:inspect repo")
	}
}

func TestTaskToolIntegration_Parallel(t *testing.T) {
	provider := &taskIntegrationProvider{parentToolCall: agent.ToolCall{
		ID:   "call_task",
		Name: "task",
		Arguments: `{"tasks":[` +
			`{"sub_agent_name":"Explorer","task":"first"},` +
			`{"sub_agent_name":"Explorer","task":"second"}` +
			`]}`,
	}}

	end := runTaskToolIntegration(t, provider)
	toolOutput := findToolOutput(end.Messages, "call_task")
	if !strings.Contains(toolOutput, "Task Explorer:\nsub:first") {
		t.Fatalf("expected first task output, got %q", toolOutput)
	}
	if !strings.Contains(toolOutput, "Task Explorer:\nsub:second") {
		t.Fatalf("expected second task output, got %q", toolOutput)
	}
}

func TestTaskToolIntegration_RejectsNonSubAgent(t *testing.T) {
	provider := &taskIntegrationProvider{parentToolCall: agent.ToolCall{
		ID:        "call_task",
		Name:      "task",
		Arguments: `{"tasks":[{"sub_agent_name":"Build","task":"inspect"}]}`,
	}}

	end := runTaskToolIntegration(t, provider)
	toolOutput := findToolOutput(end.Messages, "call_task")
	if !strings.Contains(toolOutput, "not a sub-agent") {
		t.Fatalf("unexpected output: %q", toolOutput)
	}
}

func TestTaskToolIntegration_SubAgentError(t *testing.T) {
	provider := &taskIntegrationProvider{
		parentToolCall: agent.ToolCall{
			ID:        "call_task",
			Name:      "task",
			Arguments: `{"tasks":[{"sub_agent_name":"Explorer","task":"inspect"}]}`,
		},
		failSubAgent: fmt.Errorf("provider failure"),
	}

	end := runTaskToolIntegration(t, provider)
	toolOutput := findToolOutput(end.Messages, "call_task")
	if !strings.Contains(toolOutput, "provider failure") {
		t.Fatalf("unexpected output: %q", toolOutput)
	}
}

func runTaskToolIntegration(t *testing.T, provider agent.Provider) agent.EventDataAgentEnd {
	t.Helper()

	aCtx := agent.Context{
		Model:        "test-model",
		Provider:     provider,
		SystemPrompt: "sys",
		RunID:        "run_task_test",
		Interactions: agent.NewInteractionManager(),
		Steering:     agent.NewSteeringQueue(),
		Tools: map[string]agent.Tool{
			"task": NewTaskTool(),
		},
		Prompts: []agent.Message{{Role: agent.RoleUser, Content: "run task"}},
	}

	var end agent.EventDataAgentEnd
	agent.RunAgentLoop(context.Background(), aCtx, func(e agent.Event) {
		if e.Type == agent.EventTypeAgentEnd {
			end = e.Data.(agent.EventDataAgentEnd)
		}
	})
	return end
}

func findToolOutput(messages []agent.Message, callID string) string {
	for _, msg := range messages {
		if msg.Role == agent.RoleTool && msg.CallID == callID {
			return msg.Content
		}
	}
	return ""
}
