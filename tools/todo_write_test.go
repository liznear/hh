package tools

import (
	"context"
	"testing"
)

func TestTodoWriteTool(t *testing.T) {
	args := map[string]any{
		"todo_items": []any{
			map[string]any{"content": "Ship feature", "status": "pending"},
			map[string]any{"content": "Run tests", "status": "wip"},
		},
	}

	res := NewTodoWriteTool().Handler.Handle(context.Background(), args)
	if res.IsErr {
		t.Fatalf("expected success, got error: %s", res.Data)
	}

	structured, ok := res.Result.(TodoWriteResult)
	if !ok {
		t.Fatalf("unexpected result type: %T", res.Result)
	}

	if len(structured.TodoItems) != 2 {
		t.Fatalf("todo item count = %d, want 2", len(structured.TodoItems))
	}
	if structured.TodoItems[1].Status != TodoStatusWIP {
		t.Fatalf("todo item status = %q, want %q", structured.TodoItems[1].Status, TodoStatusWIP)
	}
}

func TestTodoWriteTool_InvalidStatus(t *testing.T) {
	args := map[string]any{
		"todo_items": []any{
			map[string]any{"content": "Ship feature", "status": "in_progress"},
		},
	}

	res := NewTodoWriteTool().Handler.Handle(context.Background(), args)
	if !res.IsErr {
		t.Fatal("expected error for invalid status")
	}
}
