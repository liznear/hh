package tui

import (
	"fmt"
	"strings"
	"testing"

	"github.com/liznear/hh/tools"
	"github.com/liznear/hh/tui/session"
)

func TestFormatToolCallWidgetBody_TodoWriteFromResult(t *testing.T) {
	item := &session.ToolCallItem{
		Name:   "todo_write",
		Status: session.ToolCallStatusSuccess,
		Result: &session.ToolCallResult{
			Result: tools.TodoWriteResult{TodoItems: []tools.TodoItem{
				{Content: "a", Status: tools.TodoStatusCompleted},
				{Content: "b", Status: tools.TodoStatusCancelled},
				{Content: "c", Status: tools.TodoStatusPending},
			}},
		},
	}

	body, _ := formatToolCallWidgetBody(toolCallWidgetModel{Item: item, Width: 80}, DefaultTheme())
	if body != "TODO 2 / 3" {
		t.Fatalf("body = %q, want %q", body, "TODO 2 / 3")
	}
}

func TestFormatToolCallWidgetBody_TodoWriteFromArgs(t *testing.T) {
	item := &session.ToolCallItem{
		Name:      "todo_write",
		Status:    session.ToolCallStatusPending,
		Arguments: `{"todo_items":[{"content":"a","status":"completed"},{"content":"b","status":"pending"}]}`,
	}

	body, _ := formatToolCallWidgetBody(toolCallWidgetModel{Item: item, Width: 80}, DefaultTheme())
	if body != "TODO 1 / 2" {
		t.Fatalf("body = %q, want %q", body, "TODO 1 / 2")
	}
}

func TestFormatToolCallWidgetBody_Write(t *testing.T) {
	item := &session.ToolCallItem{
		Name:      "write",
		Status:    session.ToolCallStatusSuccess,
		Arguments: `{"path":"tmp/file.txt"}`,
		Result: &session.ToolCallResult{
			Result: tools.WriteResult{AddedLines: 3},
		},
	}

	body, _ := formatToolCallWidgetBody(toolCallWidgetModel{Item: item, Width: 80}, DefaultTheme())
	if body != "Write tmp/file.txt +3" {
		t.Fatalf("body = %q, want %q", body, "Write tmp/file.txt +3")
	}
}

func TestFormatToolCallWidgetBody_Read_UsesBeautifiedPath(t *testing.T) {
	item := &session.ToolCallItem{
		Name:      "read",
		Status:    session.ToolCallStatusPending,
		Arguments: `{"path":"/work/repo/a_folder/b_folder/c_folder/d_folder/file.txt"}`,
	}

	body, _ := formatToolCallWidgetBody(toolCallWidgetModel{Item: item, Width: 80, WorkingDir: "/work/repo"}, DefaultTheme())
	if body != "Read a/b/c/d/file.txt" {
		t.Fatalf("body = %q, want %q", body, "Read a/b/c/d/file.txt")
	}
}

func TestFormatToolCallWidgetBody_Skill(t *testing.T) {
	item := &session.ToolCallItem{
		Name:      "skill",
		Status:    session.ToolCallStatusPending,
		Arguments: `{"name":"cleanup"}`,
	}

	body, _ := formatToolCallWidgetBody(toolCallWidgetModel{Item: item, Width: 80}, DefaultTheme())
	if body != `Skill "cleanup"` {
		t.Fatalf("body = %q, want %q", body, `Skill "cleanup"`)
	}
}

func TestFormatToolCallWidgetBody_Bash(t *testing.T) {
	item := &session.ToolCallItem{
		Name:      "bash",
		Status:    session.ToolCallStatusPending,
		Arguments: `{"command":"ls -la"}`,
	}

	body, _ := formatToolCallWidgetBody(toolCallWidgetModel{Item: item, Width: 80}, DefaultTheme())
	if body != `Bash "ls -la"` {
		t.Fatalf("body = %q, want %q", body, `Bash "ls -la"`)
	}
}

func TestFormatToolCallWidgetBody_BashTruncatesLongCommand(t *testing.T) {
	command := strings.Repeat("x", 81)
	item := &session.ToolCallItem{
		Name:      "bash",
		Status:    session.ToolCallStatusPending,
		Arguments: fmt.Sprintf(`{"command":%q}`, command),
	}

	body, _ := formatToolCallWidgetBody(toolCallWidgetModel{Item: item, Width: 80}, DefaultTheme())
	wantCommand := strings.Repeat("x", 47) + "..."
	want := fmt.Sprintf(`Bash %q`, wantCommand)
	if body != want {
		t.Fatalf("body = %q, want %q", body, want)
	}
}

func TestFormatToolCallWidgetBody_BashTruncatesByWidth(t *testing.T) {
	command := strings.Repeat("x", 30)
	item := &session.ToolCallItem{
		Name:      "bash",
		Status:    session.ToolCallStatusPending,
		Arguments: fmt.Sprintf(`{"command":%q}`, command),
	}

	body, _ := formatToolCallWidgetBody(toolCallWidgetModel{Item: item, Width: 30}, DefaultTheme())
	wantCommand := strings.Repeat("x", 7) + "..."
	want := fmt.Sprintf(`Bash %q`, wantCommand)
	if body != want {
		t.Fatalf("body = %q, want %q", body, want)
	}
}

func TestFormatToolCallWidgetBody_Question(t *testing.T) {
	item := &session.ToolCallItem{
		Name:      "question",
		Status:    session.ToolCallStatusPending,
		Arguments: `{"question":{"title":"Choose deployment mode"},"options":[{"title":"safe","description":"use safe mode"}],"allow_custom_option":false}`,
	}

	body, _ := formatToolCallWidgetBody(toolCallWidgetModel{Item: item, Width: 80}, DefaultTheme())
	if body != `Question: "Choose deployment mode"` {
		t.Fatalf("body = %q, want %q", body, `Question: "Choose deployment mode"`)
	}
}

func TestRenderToolCallWidget_TaskOmitsGlobalArrow(t *testing.T) {
	theme := DefaultTheme()
	item := &session.ToolCallItem{
		Name:      "task",
		Status:    session.ToolCallStatusPending,
		Arguments: `{"tasks":[{"sub_agent_name":"Explorer","task":"Inspect architecture"}]}`,
	}

	lines := renderToolCallWidget(toolCallWidgetModel{Item: item, Width: 80}, theme)
	if len(lines) == 0 {
		t.Fatal("expected rendered lines")
	}
	if strings.HasPrefix(lines[0], "→ ") {
		t.Fatalf("unexpected global arrow prefix: %q", lines[0])
	}
}

func TestFormatToolCallWidgetBody_TaskSingle(t *testing.T) {
	item := &session.ToolCallItem{
		Name:      "task",
		Status:    session.ToolCallStatusPending,
		Arguments: `{"tasks":[{"sub_agent_name":"Explorer","task":"Inspect architecture"}]}`,
	}

	body, _ := formatToolCallWidgetBody(toolCallWidgetModel{Item: item, Width: 80}, DefaultTheme())
	if body != "• Task Explorer: Inspect architecture" {
		t.Fatalf("body = %q, want %q", body, "• Task Explorer: Inspect architecture")
	}
}

func TestFormatToolCallWidgetBody_TaskSingle_TruncatesLongTask(t *testing.T) {
	item := &session.ToolCallItem{
		Name:      "task",
		Status:    session.ToolCallStatusPending,
		Arguments: `{"tasks":[{"sub_agent_name":"Explorer","task":"This is a very very very very very very very long task description"}]}`,
	}

	body, _ := formatToolCallWidgetBody(toolCallWidgetModel{Item: item, Width: 50}, DefaultTheme())
	if !strings.Contains(body, "...") {
		t.Fatalf("expected truncated task with ellipsis, got %q", body)
	}
}

func TestFormatToolCallWidgetBody_TaskMultiple(t *testing.T) {
	item := &session.ToolCallItem{
		Name:      "task",
		Status:    session.ToolCallStatusPending,
		Arguments: `{"tasks":[{"sub_agent_name":"Explorer","task":"Inspect architecture"},{"sub_agent_name":"Explorer","task":"Find tests"}]}`,
	}

	body, _ := formatToolCallWidgetBody(toolCallWidgetModel{Item: item, Width: 80}, DefaultTheme())
	want := "• Task Explorer: Inspect architecture\n• Task Explorer: Find tests"
	if body != want {
		t.Fatalf("body = %q, want %q", body, want)
	}
}

func TestFormatToolCallWidgetBody_TaskMultiple_WithErrorDetail(t *testing.T) {
	item := &session.ToolCallItem{
		Name:   "task",
		Status: session.ToolCallStatusSuccess,
		Result: &session.ToolCallResult{
			Result: tools.TaskResult{Tasks: []tools.TaskTaskResult{
				{SubAgentName: "Explorer", Task: "Inspect architecture", Status: tools.TaskTaskStatusSuccess},
				{SubAgentName: "Explorer", Task: "Find tests", Status: tools.TaskTaskStatusError, Error: "failed to run sub-agent"},
			}},
		},
	}

	body, _ := formatToolCallWidgetBody(toolCallWidgetModel{Item: item, Width: 100}, DefaultTheme())
	want := "✓ Task Explorer: Inspect architecture\n⨯ Task Explorer: Find tests\n  |- failed to run sub-agent"
	if body != want {
		t.Fatalf("body = %q, want %q", body, want)
	}
}

func TestRenderUserMessageWidget_QueuedBadge(t *testing.T) {
	m := newInputTestModel()
	lines := m.renderUserMessageWidget(&session.UserMessage{Content: "steer", Queued: true}, 80)
	joined := strings.Join(lines, "\n")
	if !strings.Contains(joined, "Queued") {
		t.Fatalf("expected queued badge, got %q", joined)
	}
}
