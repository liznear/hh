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
	Role    Role   `json:"Role"`
	Content string `json:"Content"`
	// Only for Assistant when returning tool calls.
	ToolCalls []ToolCall `json:"ToolCalls,omitempty"`
	// Only for Tool
	CallID string `json:"CallID"`
}
