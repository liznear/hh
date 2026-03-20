package agent

import "context"

type Tool struct {
	Name        string
	Description string
	Schema      map[string]any
	Handler     func(context.Context, string) ToolResult
}

type ToolResult struct {
	IsErr       bool
	ContentType string
	Data        string
}

type ToolCall struct {
	ID        string
	Name      string
	Arguments string
}
