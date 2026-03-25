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
	// InternalState is additional context sent to LLM but not displayed in TUI.
	// Used for things like todo items that influence the agent but shouldn't clutter the UI.
	InternalState string `json:"InternalState,omitempty"`
	// Only for Assistant when returning tool calls.
	ToolCalls []ToolCall `json:"ToolCalls,omitempty"`
	// Only for Tool
	CallID string `json:"CallID"`
}
