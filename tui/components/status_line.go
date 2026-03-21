package components

import (
	"fmt"
	"time"

	"github.com/charmbracelet/lipgloss"
)

const defaultStatusHint = "Enter to send, Shift+Enter newline, PgUp/PgDn scroll, q quit"

type StatusLineParams struct {
	Busy          bool
	ShowRunResult bool
	SpinnerView   string
	Elapsed       time.Duration
	InfoColor     lipgloss.Color
	MutedColor    lipgloss.Color
	SuccessColor  lipgloss.Color
}

func RenderStatusLine(params StatusLineParams) string {
	if params.Busy {
		spinnerView := lipgloss.NewStyle().Foreground(params.InfoColor).Render(params.SpinnerView)
		durationView := lipgloss.NewStyle().Foreground(params.MutedColor).Render(" " + formatElapsedSeconds(params.Elapsed))
		return spinnerView + durationView
	}

	if params.ShowRunResult {
		checkView := lipgloss.NewStyle().Foreground(params.SuccessColor).Render("✓")
		durationView := lipgloss.NewStyle().Foreground(params.MutedColor).Render(" " + formatElapsedSeconds(params.Elapsed))
		return checkView + durationView
	}

	return lipgloss.NewStyle().Foreground(params.MutedColor).Render(defaultStatusHint)
}

func formatElapsedSeconds(d time.Duration) string {
	if d < 0 {
		d = 0
	}
	return fmt.Sprintf("%ds", int(d.Truncate(time.Second)/time.Second))
}
