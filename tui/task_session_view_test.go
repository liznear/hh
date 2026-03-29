package tui

import (
	"strings"
	"testing"

	tea "charm.land/bubbletea/v2"
	"github.com/charmbracelet/x/ansi"
	"github.com/liznear/hh/agent"
)

func TestBuildTaskSessionState_PreservesConversationShape(t *testing.T) {
	target := taskLineClickTarget{
		SubAgentName: "Explorer",
		Task:         "Inspect architecture",
		AgentMessages: []agent.Message{
			{Role: agent.RoleUser, Content: "Inspect architecture"},
			{Role: agent.RoleAssistant, ToolCalls: []agent.ToolCall{{ID: "call_1", Name: "read", Arguments: `{"path":"README.md"}`}}},
			{Role: agent.RoleTool, CallID: "call_1", Content: "file content"},
			{Role: agent.RoleAssistant, Content: "Summary"},
		},
	}

	state, _ := buildTaskSessionState(target)
	items := state.AllItems()
	if len(items) < 5 {
		t.Fatalf("expected rendered session items, got %d", len(items))
	}
}

func TestRenderTaskSessionMessageList_UsesNormalSessionRendering(t *testing.T) {
	m := newTestModel()
	m.width = 120
	m.height = 40
	m.syncLayout()

	target := taskLineClickTarget{
		SubAgentName: "Explorer",
		Task:         "Inspect architecture",
		AgentMessages: []agent.Message{
			{Role: agent.RoleUser, Content: "Inspect architecture"},
			{Role: agent.RoleAssistant, Content: "Here is the plan"},
		},
	}
	m.openTaskSessionView(target)

	rendered := m.renderTaskSessionMessageList(m.messageWidth, m.messageHeight)
	plain := ansi.Strip(rendered)
	if !strings.Contains(plain, "Inspect architecture") {
		t.Fatalf("expected user content in render, got %q", rendered)
	}
	if !strings.Contains(plain, "Here is the") || !strings.Contains(plain, "plan") {
		t.Fatalf("expected assistant content in render, got %q", rendered)
	}
}

func TestHandleTaskSessionViewKey_EscClosesView(t *testing.T) {
	m := newTestModel()
	m.taskSessionView = &taskSessionViewState{}

	if !m.handleTaskSessionViewKey(tea.KeyPressMsg(tea.Key{Code: tea.KeyEscape})) {
		t.Fatal("expected key to be handled")
	}
	if m.taskSessionView != nil {
		t.Fatal("expected task session view to be closed")
	}
}
