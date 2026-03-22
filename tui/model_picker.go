package tui

import (
	"fmt"
	"strings"

	tea "charm.land/bubbletea/v2"
	"github.com/charmbracelet/lipgloss"
)

func (m *model) openModelPicker() {
	availableModels := m.config.AvailableModels()
	if len(availableModels) == 0 {
		m.modelPicker = nil
		return
	}

	index := 0
	for i, modelName := range availableModels {
		if modelName == m.modelName {
			index = i
			break
		}
	}
	m.modelPicker = &modelPickerState{index: index}
}

func (m *model) closeModelPicker() {
	m.modelPicker = nil
}

func (m *model) handleModelPickerKey(msg tea.KeyPressMsg) bool {
	if m.modelPicker == nil {
		return false
	}

	key := msg.Key()
	switch key.Code {
	case tea.KeyEscape:
		m.closeModelPicker()
		m.refreshViewport()
		return true
	case tea.KeyEnter:
		availableModels := m.config.AvailableModels()
		if len(availableModels) > 0 {
			selected := availableModels[m.modelPicker.index]
			m.switchModel(selected)
		}
		m.closeModelPicker()
		m.refreshViewport()
		return true
	case tea.KeyUp:
		m.moveModelPicker(-1)
		m.refreshViewport()
		return true
	case tea.KeyDown:
		m.moveModelPicker(1)
		m.refreshViewport()
		return true
	}

	switch strings.ToLower(msg.String()) {
	case "k":
		m.moveModelPicker(-1)
		m.refreshViewport()
		return true
	case "j":
		m.moveModelPicker(1)
		m.refreshViewport()
		return true
	}

	return true
}

func (m *model) moveModelPicker(delta int) {
	availableModels := m.config.AvailableModels()
	if m.modelPicker == nil || len(availableModels) == 0 {
		return
	}
	next := m.modelPicker.index + delta
	if next < 0 {
		next = len(availableModels) - 1
	}
	if next >= len(availableModels) {
		next = 0
	}
	m.modelPicker.index = next
}

func (m *model) switchModel(modelName string) {
	modelName = strings.TrimSpace(modelName)
	if modelName == "" || modelName == m.modelName {
		return
	}

	m.modelName = modelName
	m.session.SetModel(modelName)
	m.runtime.contextWindowTotal = m.contextWindowTotalFor(strings.TrimSpace(modelName))
	m.runtime.contextWindowUsed = 0
	if m.runner != nil {
		m.runner.SetModel(modelName)
	}
	m.persistMeta()
}

func (m *model) renderModelPickerDialog(width, height int) string {
	availableModels := m.config.AvailableModels()
	if len(availableModels) == 0 || m.modelPicker == nil {
		return ""
	}

	selection := m.modelPicker.index
	if selection < 0 {
		selection = 0
	}
	if selection >= len(availableModels) {
		selection = len(availableModels) - 1
	}

	selectedStyle := lipgloss.NewStyle().Bold(true).Foreground(m.theme.Info())
	mutedStyle := lipgloss.NewStyle().Foreground(m.theme.Muted())
	lines := []string{"Pick a model", mutedStyle.Render("Enter to apply  Esc to cancel")}

	maxRows := max(1, height-8)
	start := 0
	if selection >= maxRows {
		start = selection - maxRows + 1
	}
	end := min(len(availableModels), start+maxRows)

	for i := start; i < end; i++ {
		prefix := "  "
		line := availableModels[i]
		if i == selection {
			prefix = "› "
			line = selectedStyle.Render(line)
		}
		lines = append(lines, prefix+line)
	}

	if end < len(availableModels) {
		lines = append(lines, mutedStyle.Render(fmt.Sprintf("... %d more", len(availableModels)-end)))
	}

	boxWidth := min(max(40, width-8), width-2)
	if boxWidth < 20 {
		boxWidth = width
	}

	dialog := lipgloss.NewStyle().
		Width(boxWidth).
		Padding(1).
		Border(lipgloss.RoundedBorder()).
		BorderForeground(m.theme.Info()).
		Render(strings.Join(lines, "\n"))

	return lipgloss.Place(width, height, lipgloss.Center, lipgloss.Center, dialog)
}
