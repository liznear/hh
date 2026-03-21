package agent

import (
	"context"
	"fmt"
)

type Tool struct {
	Name        string
	Description string
	Schema      map[string]any
	Handler     ToolHandler
}

type ToolHandler interface {
	Handle(ctx context.Context, params map[string]any) ToolResult

	// Returns an optional state. This state can be used as part
	// of the context.
	State() fmt.Stringer
}

type ToolResult struct {
	IsErr       bool
	Result      any
	ContentType string
	Data        string
}

type ToolCall struct {
	ID        string
	Name      string
	Arguments string
}

type FuncToolHandler func(context.Context, map[string]any) ToolResult

func (f FuncToolHandler) Handle(ctx context.Context, params map[string]any) ToolResult {
	return f(ctx, params)
}

func (f FuncToolHandler) State() fmt.Stringer {
	return nil
}

var _ ToolHandler = (FuncToolHandler)(nil)
