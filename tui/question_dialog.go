package tui

import (
	"fmt"
	"strings"
	"time"

	tea "charm.land/bubbletea/v2"
	"github.com/charmbracelet/lipgloss"
	"github.com/liznear/hh/agent"
)

type questionDialogState struct {
	request       agent.InteractionRequest
	selectedIndex int
	typingCustom  bool
	customInput   string
	errorMessage  string
}

func (m *model) openQuestionDialog(req agent.InteractionRequest) {
	if len(req.Options) == 0 {
		return
	}
	m.runtime.questionDialog = &questionDialogState{request: req}
	m.runtime.questionPromptedAt = time.Now()
}

func (m *model) closeQuestionDialog() {
	m.runtime.questionDialog = nil
}

func (m *model) handleQuestionDialogKey(msg tea.KeyPressMsg) bool {
	if m.runtime.questionDialog == nil {
		return false
	}

	dlg := m.runtime.questionDialog
	key := msg.Key()

	if dlg.typingCustom {
		switch key.Code {
		case tea.KeyEscape:
			return m.dismissQuestionDialog()
		case tea.KeyBackspace:
			if len(dlg.customInput) > 0 {
				runes := []rune(dlg.customInput)
				dlg.customInput = string(runes[:len(runes)-1])
			}
			return true
		case tea.KeyEnter:
			custom := strings.TrimSpace(dlg.customInput)
			if custom == "" {
				dlg.errorMessage = "Custom answer cannot be empty"
				m.runtime.questionValidationErrors++
				return true
			}
			if m.runner == nil {
				dlg.errorMessage = "runner unavailable"
				m.runtime.questionValidationErrors++
				return true
			}
			err := m.runner.SubmitInteractionResponse(agent.InteractionResponse{
				InteractionID: dlg.request.InteractionID,
				RunID:         dlg.request.RunID,
				CustomText:    custom,
			})
			if err != nil {
				dlg.errorMessage = err.Error()
				m.runtime.questionValidationErrors++
				return true
			}
			m.runtime.questionSubmittedCount++
			if !m.runtime.questionPromptedAt.IsZero() {
				m.runtime.questionLastLatency = time.Since(m.runtime.questionPromptedAt)
			}
			dlg.errorMessage = ""
			return true
		}

		s := msg.String()
		if len(s) == 1 {
			dlg.customInput += s
			dlg.errorMessage = ""
			return true
		}
		return true
	}

	optionCount := len(dlg.request.Options)
	totalItems := optionCount
	if dlg.request.AllowCustomOption {
		totalItems++
	}

	switch key.Code {
	case tea.KeyUp:
		dlg.selectedIndex--
		if dlg.selectedIndex < 0 {
			dlg.selectedIndex = totalItems - 1
		}
		dlg.errorMessage = ""
		return true
	case tea.KeyDown:
		dlg.selectedIndex++
		if dlg.selectedIndex >= totalItems {
			dlg.selectedIndex = 0
		}
		dlg.errorMessage = ""
		return true
	case tea.KeyEnter:
		if dlg.request.AllowCustomOption && dlg.selectedIndex == optionCount {
			dlg.typingCustom = true
			dlg.errorMessage = ""
			return true
		}

		selected := dlg.request.Options[dlg.selectedIndex]
		if m.runner == nil {
			dlg.errorMessage = "runner unavailable"
			m.runtime.questionValidationErrors++
			return true
		}
		err := m.runner.SubmitInteractionResponse(agent.InteractionResponse{
			InteractionID:    dlg.request.InteractionID,
			RunID:            dlg.request.RunID,
			SelectedOptionID: selected.ID,
		})
		if err != nil {
			dlg.errorMessage = err.Error()
			m.runtime.questionValidationErrors++
			return true
		}
		m.runtime.questionSubmittedCount++
		if !m.runtime.questionPromptedAt.IsZero() {
			m.runtime.questionLastLatency = time.Since(m.runtime.questionPromptedAt)
		}
		dlg.errorMessage = ""
		return true
	case tea.KeyEscape:
		return m.dismissQuestionDialog()
	}

	switch strings.ToLower(msg.String()) {
	case "k":
		dlg.selectedIndex--
		if dlg.selectedIndex < 0 {
			dlg.selectedIndex = totalItems - 1
		}
		dlg.errorMessage = ""
		return true
	case "j":
		dlg.selectedIndex++
		if dlg.selectedIndex >= totalItems {
			dlg.selectedIndex = 0
		}
		dlg.errorMessage = ""
		return true
	}

	return true
}

func (m *model) dismissQuestionDialog() bool {
	dlg := m.runtime.questionDialog
	if dlg == nil {
		return false
	}
	if m.runner == nil {
		dlg.errorMessage = "runner unavailable"
		m.runtime.questionValidationErrors++
		return true
	}
	err := m.runner.DismissInteraction(dlg.request.InteractionID, dlg.request.RunID)
	if err != nil {
		dlg.errorMessage = err.Error()
		m.runtime.questionValidationErrors++
		return true
	}
	m.closeQuestionDialog()
	return true
}

func (m *model) renderQuestionDialog(width, height int) string {
	dlg := m.runtime.questionDialog
	if dlg == nil {
		return ""
	}

	maxRows := max(1, height-8)
	lines := []string{"", lipgloss.NewStyle().Bold(true).Render(dlg.request.Title), ""}
	if content := strings.TrimSpace(dlg.request.Content); content != "" {
		lines = append(lines, content, "")
	}

	for idx, opt := range dlg.request.Options {
		prefix := " "
		if dlg.selectedIndex == idx && !dlg.typingCustom {
			prefix = "›"
		}
		lines = append(lines, fmt.Sprintf("%s %d. %s", prefix, idx+1, opt.Title))
		lines = append(lines, fmt.Sprintf("   %s", opt.Description))
	}

	if dlg.request.AllowCustomOption {
		customIndex := len(dlg.request.Options) + 1
		prefix := " "
		if dlg.selectedIndex == customIndex-1 || dlg.typingCustom {
			prefix = "›"
		}
		lines = append(lines, fmt.Sprintf("%s %d. Type your own answer", prefix, customIndex))
		if dlg.typingCustom {
			lines = append(lines, fmt.Sprintf("   > %s", dlg.customInput))
			lines = append(lines, "   Press Enter again to submit")
		} else {
			lines = append(lines, "   Once selected, type and press Enter")
		}
	}

	if msg := strings.TrimSpace(dlg.errorMessage); msg != "" {
		lines = append(lines, "", lipgloss.NewStyle().Foreground(m.theme.Color(ThemeColorSidebarErrorForeground)).Render(msg))
	}

	lines = append(lines, "")
	if len(lines) > maxRows {
		lines = lines[:maxRows]
	}

	boxWidth := min(max(56, width-8), width-2)
	if boxWidth < 24 {
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
