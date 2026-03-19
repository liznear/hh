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

type Tool struct{}

type ToolCall struct{}
