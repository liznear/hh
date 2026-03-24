package tui

import (
	"fmt"

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

	m.refreshViewport()
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
	m.markdownCache = map[string]string{}
	m.itemRenderCache = map[uintptr]itemRenderCacheEntry{}
	m.persistState()
}
