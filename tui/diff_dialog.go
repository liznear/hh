package tui

import (
	"fmt"
	"strings"

	tea "charm.land/bubbletea/v2"
	"github.com/charmbracelet/lipgloss"
)

type diffDialogState struct {
	Title        string
	OldContent   string
	NewContent   string
	FilePath     string
	ScrollOffset int
	// Cached rendered lines (re-render only when width changes)
	renderedLines []string
	lastWidth     int
}

func (m *model) openDiffDialog(title string, oldContent, newContent, filePath string) {
	title = strings.TrimSpace(title)
	if title == "" {
		title = "Diff"
	}
	m.diffDialog = &diffDialogState{
		Title:        title,
		OldContent:   oldContent,
		NewContent:   newContent,
		FilePath:     strings.TrimSpace(filePath),
		ScrollOffset: 0,
	}
}

func (m *model) closeDiffDialog() {
	m.diffDialog = nil
}

func (m *model) handleDiffDialogKey(msg tea.KeyPressMsg) bool {
	dlg := m.diffDialog
	if dlg == nil {
		return false
	}

	switch msg.Key().Code {
	case tea.KeyEscape, tea.KeyEnter:
		m.closeDiffDialog()
		return true
	case tea.KeyUp:
		dlg.ScrollOffset = max(0, dlg.ScrollOffset-1)
		return true
	case tea.KeyDown:
		dlg.ScrollOffset++
		return true
	case tea.KeyPgUp:
		dlg.ScrollOffset = max(0, dlg.ScrollOffset-10)
		return true
	case tea.KeyPgDown:
		dlg.ScrollOffset += 10
		return true
	case tea.KeyHome:
		dlg.ScrollOffset = 0
		return true
	}

	switch strings.ToLower(msg.String()) {
	case "q":
		m.closeDiffDialog()
		return true
	case "k":
		dlg.ScrollOffset = max(0, dlg.ScrollOffset-1)
		return true
	case "j":
		dlg.ScrollOffset++
		return true
	}
	return true
}

func (m *model) renderDiffDialog(width, height int) string {
	dlg := m.diffDialog
	if dlg == nil {
		return ""
	}

	boxWidth := min(max(100, width-8), width-2)
	if boxWidth < 30 {
		boxWidth = width
	}
	bodyWidth := max(1, boxWidth-4)
	maxBodyRows := max(1, height-10)

	// Only re-render diff if width changed
	if dlg.renderedLines == nil || dlg.lastWidth != bodyWidth {
		dlg.renderedLines = RenderSplitDiff(dlg.OldContent, dlg.NewContent, dlg.FilePath, bodyWidth, m.theme)
		if len(dlg.renderedLines) == 0 {
			dlg.renderedLines = []string{"(no diff)"}
		}
		dlg.lastWidth = bodyWidth
	}

	maxOffset := max(0, len(dlg.renderedLines)-maxBodyRows)
	if dlg.ScrollOffset > maxOffset {
		dlg.ScrollOffset = maxOffset
	}
	if dlg.ScrollOffset < 0 {
		dlg.ScrollOffset = 0
	}

	end := min(len(dlg.renderedLines), dlg.ScrollOffset+maxBodyRows)
	visibleDiff := dlg.renderedLines[dlg.ScrollOffset:end]

	muted := lipgloss.NewStyle().Foreground(m.theme.Color(ThemeColorModelPickerMutedForeground))
	scrollLine := ""
	if len(dlg.renderedLines) > maxBodyRows {
		scrollLine = muted.Render(fmt.Sprintf("Lines %d-%d / %d", dlg.ScrollOffset+1, end, len(dlg.renderedLines)))
	}

	lines := make([]string, 0, 5+len(visibleDiff))
	lines = append(lines,
		lipgloss.NewStyle().Bold(true).Render(dlg.Title),
		muted.Render("Scroll: j/k, up/down, pgup/pgdown, home"),
		muted.Render("Close: Esc/Enter/q"),
	)
	if scrollLine != "" {
		lines = append(lines, scrollLine)
	}
	lines = append(lines, "")
	lines = append(lines, visibleDiff...)

	dialog := lipgloss.NewStyle().
		Width(boxWidth).
		Padding(1).
		Border(lipgloss.RoundedBorder()).
		BorderForeground(m.theme.Color(ThemeColorModelPickerBorderForeground)).
		Render(strings.Join(lines, "\n"))

	return lipgloss.Place(width, height, lipgloss.Center, lipgloss.Center, dialog)
}
