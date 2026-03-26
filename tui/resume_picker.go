package tui

import (
	"fmt"
	"strings"

	tea "charm.land/bubbletea/v2"
	"github.com/charmbracelet/lipgloss"
	"github.com/liznear/hh/tui/session"
)

type resumePickerState struct {
	index    int
	sessions []session.Meta
}

func (m *model) openResumePicker() error {
	if m.storage == nil {
		return fmt.Errorf("session storage unavailable")
	}

	metas, err := m.storage.List()
	if err != nil {
		return fmt.Errorf("failed to list sessions: %w", err)
	}
	if len(metas) == 0 {
		m.resumePicker = nil
		m.addItem(&session.AssistantMessage{Content: "No sessions found for current path."})
		m.refreshViewport()
		return nil
	}

	m.modelPicker = nil
	m.resumePicker = &resumePickerState{index: 0, sessions: metas}
	return nil
}

func (m *model) closeResumePicker() {
	m.resumePicker = nil
}

func (m *model) handleResumePickerKey(msg tea.KeyPressMsg) bool {
	if m.resumePicker == nil {
		return false
	}

	key := msg.Key()
	switch key.Code {
	case tea.KeyEscape:
		m.closeResumePicker()
		m.refreshViewport()
		return true
	case tea.KeyEnter:
		if err := m.selectResumeSession(); err != nil {
			m.addItem(&session.ErrorItem{Message: err.Error()})
		}
		m.closeResumePicker()
		m.refreshViewport()
		return true
	case tea.KeyUp:
		m.moveResumePicker(-1)
		m.refreshViewport()
		return true
	case tea.KeyDown:
		m.moveResumePicker(1)
		m.refreshViewport()
		return true
	}

	switch strings.ToLower(msg.String()) {
	case "k":
		m.moveResumePicker(-1)
		m.refreshViewport()
		return true
	case "j":
		m.moveResumePicker(1)
		m.refreshViewport()
		return true
	}

	return true
}

func (m *model) moveResumePicker(delta int) {
	if m.resumePicker == nil || len(m.resumePicker.sessions) == 0 {
		return
	}
	next := m.resumePicker.index + delta
	if next < 0 {
		next = len(m.resumePicker.sessions) - 1
	}
	if next >= len(m.resumePicker.sessions) {
		next = 0
	}
	m.resumePicker.index = next
}

func (m *model) selectResumeSession() error {
	if m.resumePicker == nil || len(m.resumePicker.sessions) == 0 {
		return nil
	}

	selection := m.resumePicker.index
	if selection < 0 {
		selection = 0
	}
	if selection >= len(m.resumePicker.sessions) {
		selection = len(m.resumePicker.sessions) - 1
	}

	selected := m.resumePicker.sessions[selection]
	loaded, err := m.storage.Load(selected.ID)
	if err != nil {
		return fmt.Errorf("failed to load session: %w", err)
	}

	m.session = loaded
	if strings.TrimSpace(loaded.CurrentModel) != "" {
		m.modelName = loaded.CurrentModel
		if m.runner != nil {
			m.runner.SetModel(loaded.CurrentModel)
		}
	}
	m.toolCalls = map[string]*session.ToolCallItem{}
	m.listOffsetIdx = 0
	m.listOffsetLine = 0
	m.autoScroll = true
	m.showRunResult = false
	m.viewportDirty = false
	m.itemRenderCache = map[uintptr]itemRenderCacheEntry{}
	return nil
}

func (m *model) renderResumePickerDialog(width, height int) string {
	if m.resumePicker == nil || len(m.resumePicker.sessions) == 0 {
		return ""
	}

	selection := m.resumePicker.index
	if selection < 0 {
		selection = 0
	}
	if selection >= len(m.resumePicker.sessions) {
		selection = len(m.resumePicker.sessions) - 1
	}

	selectedStyle := lipgloss.NewStyle().Bold(true).Foreground(m.theme.Color(ThemeColorModelPickerSelectedForeground))
	mutedStyle := lipgloss.NewStyle().Foreground(m.theme.Color(ThemeColorModelPickerMutedForeground))
	lines := []string{"Resume a session", mutedStyle.Render("Enter to load  Esc to cancel")}

	maxRows := max(1, height-8)
	start := 0
	if selection >= maxRows {
		start = selection - maxRows + 1
	}
	end := min(len(m.resumePicker.sessions), start+maxRows)

	for i := start; i < end; i++ {
		prefix := "  "
		title := strings.TrimSpace(m.resumePicker.sessions[i].Title)
		if title == "" {
			title = "Untitled Session"
		}
		line := title
		if i == selection {
			prefix = "› "
			line = selectedStyle.Render(line)
		}
		lines = append(lines, prefix+line)
	}

	if end < len(m.resumePicker.sessions) {
		lines = append(lines, mutedStyle.Render(fmt.Sprintf("... %d more", len(m.resumePicker.sessions)-end)))
	}

	boxWidth := min(max(40, width-8), width-2)
	if boxWidth < 20 {
		boxWidth = width
	}

	dialog := lipgloss.NewStyle().
		Width(boxWidth).
		Padding(1).
		Border(lipgloss.RoundedBorder()).
		BorderForeground(m.theme.Color(ThemeColorModelPickerBorderForeground)).
		Render(strings.Join(lines, "\n"))

	return lipgloss.Place(width, height, lipgloss.Center, lipgloss.Center, dialog)
}
