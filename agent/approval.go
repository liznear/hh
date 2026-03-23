package agent

import "context"

type ToolApprover interface {
	Approve(ctx context.Context, toolName string, params map[string]any) error
}
