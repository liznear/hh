package agent

type Role int

const (
	RoleUnknown   = 0
	RoleSystem    = 1
	RoleUser      = 2
	RoleAssistant = 3
	RoleTool      = 4
)

type Message struct {
	Role    Role
	Content string
	// Only for Tool
	CallID string
}

type ToolType string

const (
	ToolTypeFunction ToolType = "function"
)

type Tool struct {
	Type     ToolType
	Function ToolFunction
}

type ToolFunction struct {
	Name        string
	Description string
	Parameters  map[string]any
}

type ToolCallType string

const (
	ToolCallTypeFunction ToolCallType = "function"
)

type ToolCall struct {
	ID       string
	Type     ToolCallType
	Function ToolCallFunction
}

type ToolCallFunction struct {
	Name      string
	Arguments string
}
