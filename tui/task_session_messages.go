package tui

import (
	"strings"

	"github.com/liznear/hh/agent"
	"github.com/liznear/hh/tui/session"
)

func taskSessionMessagesFromState(state *session.State) []agent.Message {
	if state == nil {
		return nil
	}
	var out []agent.Message
	for _, item := range state.AllItems() {
		switch v := item.(type) {
		case *session.UserMessage:
			out = append(out, agent.Message{Role: agent.RoleUser, Content: strings.TrimSpace(v.Content)})
		case *session.AssistantMessage:
			out = append(out, agent.Message{Role: agent.RoleAssistant, Content: strings.TrimSpace(v.Content)})
		case *session.ToolCallItem:
			if v.Status == session.ToolCallStatusPending {
				out = append(out, agent.Message{Role: agent.RoleAssistant, ToolCalls: []agent.ToolCall{{ID: v.ID, Name: v.Name, Arguments: v.Arguments}}})
				continue
			}
			content := ""
			if v.Result != nil {
				if strings.TrimSpace(v.Result.Data) != "" {
					content = strings.TrimSpace(v.Result.Data)
				} else if s, ok := v.Result.Result.(string); ok {
					content = strings.TrimSpace(s)
				}
			}
			out = append(out, agent.Message{Role: agent.RoleTool, CallID: v.ID, Content: content})
		case *session.ThinkingBlock:
			out = append(out, agent.Message{Role: agent.RoleAssistant, Content: strings.TrimSpace(v.Content)})
		}
	}
	return out
}
