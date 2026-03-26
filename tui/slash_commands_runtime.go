package tui

import (
	"context"
	"fmt"

	tea "charm.land/bubbletea/v2"
	"github.com/liznear/hh/agent"
	"github.com/liznear/hh/tui/commands"
	"github.com/liznear/hh/tui/session"
)

func (m *model) handleSlashCommand(prompt string) bool {
	inv, ok := commands.ParseInvocation(prompt)
	if !ok {
		return false
	}

	if len(m.slashCommands) == 0 {
		m.slashCommands = commands.BuiltIn()
	}

	cmd, exists := m.slashCommands[inv.Name]
	if !exists {
		m.addItem(&session.ErrorItem{Message: fmt.Sprintf("unknown slash command: /%s", inv.Name)})
		m.refreshViewport()
		return true
	}

	if err := m.executeSlashCommand(cmd, inv); err != nil {
		m.addItem(&session.ErrorItem{Message: err.Error()})
		m.refreshViewport()
		return true
	}
	return true
}

func (m *model) executeSlashCommand(cmd commands.Command, inv commands.Invocation) error {
	switch cmd.Action {
	case commands.ActionNewSession:
		if len(inv.Args) > 0 {
			return fmt.Errorf("/%s does not accept arguments", inv.Name)
		}
		m.startNewSession()
		return nil
	case commands.ActionModelPicker:
		if len(inv.Args) > 0 {
			return fmt.Errorf("/%s does not accept arguments", inv.Name)
		}
		if len(m.config.AvailableModels()) == 0 {
			return fmt.Errorf("no models available from config")
		}
		m.openModelPicker()
		return nil
	case commands.ActionResumeSession:
		if len(inv.Args) > 0 {
			return fmt.Errorf("/%s does not accept arguments", inv.Name)
		}
		return m.openResumePicker()
	case commands.ActionBTW:
		if inv.ArgsRaw == "" {
			return fmt.Errorf("/%s requires a prompt", inv.Name)
		}
		return m.startBTWRun(inv.ArgsRaw)
	default:
		return fmt.Errorf("unsupported slash command action: %s", cmd.Action)
	}
}

func (m *model) startNewSession() {
	m.session = session.NewState(m.modelName)
	m.toolCalls = map[string]*session.ToolCallItem{}
	m.listOffsetIdx = 0
	m.listOffsetLine = 0
	m.autoScroll = true
	m.showRunResult = false
	m.viewportDirty = false
	m.itemRenderCache = map[uintptr]itemRenderCacheEntry{}
	m.ephemeralItems = nil
	m.contextWindowUsed = 0
	m.persistState()
}

func (m *model) startBTWRun(prompt string) error {
	if m.runner == nil {
		return fmt.Errorf("runner unavailable")
	}

	turn := m.session.CurrentTurn()
	if turn == nil {
		return fmt.Errorf("no active turn")
	}

	turnID := turn.ID
	turnIdx := len(turn.Items) - 1
	if turnIdx < 0 {
		turnIdx = 0
	}

	m.input.SetValue("")
	m.autoScroll = true

	// Create a single BTW exchange item that holds both Q and A
	exchange := &session.BTWExchange{Question: prompt}
	m.ephemeralItems = append(m.ephemeralItems, ephemeralItem{
		turnID:     turnID,
		afterIndex: turnIdx,
		item:       exchange,
	})
	m.refreshViewport()

	runCtx := context.Background()
	cmd := m.startBTWStream(runCtx, prompt, turnID, turnIdx, exchange)
	// Store the cmd to be returned to the tea runtime
	m.btwStreamCmd = cmd
	return nil
}

func (m *model) startBTWStream(ctx context.Context, prompt, turnID string, turnIdx int, exchange *session.BTWExchange) tea.Cmd {
	return func() tea.Msg {
		messages := buildBTWMessages(m.session)
		messages = append(messages, agent.Message{Role: agent.RoleUser, Content: prompt})

		ch := make(chan tea.Msg)
		go func() {
			defer close(ch)
			err := m.runner.RunSideQuery(ctx, messages, prompt, func(e agent.Event) {
				ch <- agentEventMsg{event: e, btw: true, btwTurnID: turnID, btwItem: exchange}
			})
			ch <- btwRunDoneMsg{turnID: turnID, err: err}
		}()
		return btwStreamStartedMsg{ch: ch}
	}
}

func buildBTWMessages(state *session.State) []agent.Message {
	var messages []agent.Message

	for _, turn := range state.Turns {
		if turn == nil {
			continue
		}
		for _, item := range turn.Items {
			msg := sessionItemToMessage(item)
			if msg.Role != agent.RoleUnknown {
				messages = append(messages, msg)
			}
		}
	}

	return messages
}

func sessionItemToMessage(item session.Item) agent.Message {
	switch i := item.(type) {
	case *session.UserMessage:
		return agent.Message{Role: agent.RoleUser, Content: i.Content}
	case *session.AssistantMessage:
		return agent.Message{Role: agent.RoleAssistant, Content: i.Content}
	case *session.ThinkingBlock:
		return agent.Message{Role: agent.RoleAssistant, Content: "<thinking>" + i.Content + "</thinking>"}
	case *session.ToolCallItem:
		if i.Result != nil {
			return agent.Message{
				Role:    agent.RoleTool,
				Content: toolResultToContent(i.Result),
				CallID:  i.ID,
			}
		}
		return agent.Message{}
	default:
		return agent.Message{}
	}
}

func toolResultToContent(result *session.ToolCallResult) string {
	if result == nil {
		return ""
	}
	if result.Data != "" {
		return result.Data
	}
	if s, ok := result.Result.(string); ok {
		return s
	}
	return ""
}
