package tools

import (
	"context"
	"encoding/json"
	"fmt"

	"github.com/liznear/hh/agent"
)

type TodoStatus string

const (
	TodoStatusPending   TodoStatus = "pending"
	TodoStatusWIP       TodoStatus = "wip"
	TodoStatusCompleted TodoStatus = "completed"
	TodoStatusCancelled TodoStatus = "cancelled"
)

type TodoItem struct {
	Content string     `json:"content"`
	Status  TodoStatus `json:"status"`
}

type TodoWriteResult struct {
	TodoItems []TodoItem `json:"todo_items"`
}

func (r TodoWriteResult) Summary() string {
	return fmt.Sprintf("%d todo items", len(r.TodoItems))
}

func NewTodoWriteTool() agent.Tool {
	return agent.Tool{
		Name:        "todo_write",
		Description: "Persist todo list state",
		Schema: map[string]any{
			"type": "object",
			"properties": map[string]any{
				"todo_items": map[string]any{
					"type": "array",
					"items": map[string]any{
						"type": "object",
						"properties": map[string]any{
							"content": map[string]any{"type": "string"},
							"status": map[string]any{
								"type": "string",
								"enum": []string{
									string(TodoStatusPending),
									string(TodoStatusWIP),
									string(TodoStatusCompleted),
									string(TodoStatusCancelled),
								},
							},
						},
						"required": []string{"content", "status"},
					},
				},
			},
			"required": []string{"todo_items"},
		},
		Handler: agent.FuncToolHandler(handleTodoWrite),
	}
}

func handleTodoWrite(_ context.Context, params map[string]any) agent.ToolResult {
	rawItems, ok := params["todo_items"]
	if !ok {
		return toolErr("todo_items is required")
	}

	b, err := json.Marshal(rawItems)
	if err != nil {
		return toolErr("todo_items must be a valid array")
	}

	var items []TodoItem
	if err := json.Unmarshal(b, &items); err != nil {
		return toolErr("todo_items must be an array of todo items")
	}

	for i, item := range items {
		if item.Content == "" {
			return toolErr("todo_items[%d].content must be a non-empty string", i)
		}
		if !isValidTodoStatus(item.Status) {
			return toolErr("todo_items[%d].status must be one of: pending, wip, completed, cancelled", i)
		}
	}

	result := TodoWriteResult{TodoItems: items}
	out, err := json.Marshal(result)
	if err != nil {
		return toolErr("failed to encode todo items: %v", err)
	}

	return agent.ToolResult{
		Data:   string(out),
		Result: result,
	}
}

func isValidTodoStatus(status TodoStatus) bool {
	switch status {
	case TodoStatusPending, TodoStatusWIP, TodoStatusCompleted, TodoStatusCancelled:
		return true
	default:
		return false
	}
}
