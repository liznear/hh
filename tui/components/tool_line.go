package components

import (
	"fmt"
	"strings"

	"github.com/charmbracelet/lipgloss"
)

func RenderPendingToolCallLine(body string, spinnerView string) string {
	return fmt.Sprintf("%s %s", spinnerView, body)
}

func RenderCompletedToolCallLine(body string, success bool, width int, successColor lipgloss.Color, errorColor lipgloss.Color) []string {
	icon := "⨯"
	color := errorColor
	if success {
		icon = "✓"
		color = successColor
	}

	iconView := lipgloss.NewStyle().Foreground(color).Render(icon)
	bodyWidth := max(1, width-2)
	bodyLines := WrapLine(body, bodyWidth)
	if len(bodyLines) == 0 {
		return []string{iconView}
	}

	out := make([]string, 0, len(bodyLines))
	out = append(out, fmt.Sprintf("%s %s", iconView, bodyLines[0]))
	for _, line := range bodyLines[1:] {
		out = append(out, "  "+line)
	}
	return out
}

func WrapLine(line string, width int) []string {
	if width <= 0 {
		return []string{line}
	}
	if line == "" {
		return []string{""}
	}

	runes := []rune(line)
	ret := make([]string, 0, 1)

	for len(runes) > width {
		breakAt := width
		for i := width; i > 0; i-- {
			if runes[i-1] == ' ' || runes[i-1] == '\t' {
				breakAt = i
				break
			}
		}

		chunk := strings.TrimRight(string(runes[:breakAt]), " \t")
		if chunk == "" {
			breakAt = width
			chunk = string(runes[:breakAt])
		}
		ret = append(ret, chunk)

		runes = runes[breakAt:]
		for len(runes) > 0 && (runes[0] == ' ' || runes[0] == '\t') {
			runes = runes[1:]
		}
	}

	ret = append(ret, string(runes))
	return ret
}

func max(a, b int) int {
	if a > b {
		return a
	}
	return b
}
