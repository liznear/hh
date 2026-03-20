package agent

type Role string

const (
	RoleUnknown   Role = "unknown"
	RoleSystem    Role = "system"
	RoleUser      Role = "user"
	RoleAssistant Role = "assistant"
	RoleTool      Role = "tool"
)

type Message struct {
	Role    Role
	Content string
	// Only for Tool
	CallID string
}
