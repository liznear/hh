package tui

import (
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
