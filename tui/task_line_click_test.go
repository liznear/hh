package tui

import (
	"strings"
	"testing"

	"github.com/liznear/hh/agent"
	"github.com/liznear/hh/tools"
	"github.com/liznear/hh/tui/session"
)

func TestTaskClickTargetsFromRenderedLines_FromTaskResultMessages(t *testing.T) {
	item := &session.ToolCallItem{
		Name:   "task",
		Status: session.ToolCallStatusSuccess,
		Result: &session.ToolCallResult{Result: tools.TaskResult{Tasks: []tools.TaskTaskResult{
			{SubAgentName: "Explorer", Task: "Inspect architecture", Status: tools.TaskTaskStatusSuccess, Messages: []agent.Message{{Role: agent.RoleUser, Content: "Inspect architecture"}}},
			{SubAgentName: "Explorer", Task: "Find tests", Status: tools.TaskTaskStatusError, Error: "failed", Messages: []agent.Message{{Role: agent.RoleAssistant, Content: "nope"}}},
		}}},
	}

	m := newTestModel()
	rendered := m.renderToolCallWidget(item, 100)
	targets := m.taskClickTargetsFromRenderedLines(item, rendered)

	if len(targets) != 2 {
		t.Fatalf("targets len = %d, want 2", len(targets))
	}
	if targets[0].Task != "Inspect architecture" || len(targets[0].AgentMessages) != 1 {
		t.Fatalf("unexpected first target: %+v", targets[0])
	}
	if targets[0].TaskIndex != 0 || targets[1].TaskIndex != 1 {
		t.Fatalf("unexpected task indexes: %+v", targets)
	}
	if targets[1].Status != "error" || targets[1].Error != "failed" {
		t.Fatalf("unexpected second target: %+v", targets[1])
	}
}

func TestHandleTaskLineClick_OpensTaskSessionView(t *testing.T) {
	m := newTestModel()
	m.width = 120
	m.height = 40
	m.syncLayout()

	item := &session.ToolCallItem{
		Name:   "task",
		Status: session.ToolCallStatusSuccess,
		Result: &session.ToolCallResult{Result: tools.TaskResult{Tasks: []tools.TaskTaskResult{
			{SubAgentName: "Explorer", Task: "Inspect architecture", Status: tools.TaskTaskStatusSuccess, Messages: []agent.Message{{Role: agent.RoleAssistant, Content: "done"}}},
		}}},
	}
	m.session.StartTurn()
	m.session.AddItem(item)
	_ = m.renderMessageList(m.messageWidth, m.messageHeight)

	if len(m.taskLineClickTargets) != 1 {
		t.Fatalf("taskLineClickTargets len = %d, want 1", len(m.taskLineClickTargets))
	}
	viewLine := m.taskLineClickTargets[0].ViewLine
	clicked := m.handleTaskLineClick(appPadding+1, appPadding+viewLine)
	if !clicked {
		t.Fatal("expected click to be handled")
	}
	if m.taskSessionView == nil {
		t.Fatal("expected task session view to open")
	}
	if !strings.Contains(m.taskSessionView.Task, "Inspect architecture") {
		t.Fatalf("unexpected session task: %q", m.taskSessionView.Task)
	}
}
