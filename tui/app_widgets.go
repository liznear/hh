package tui

import (
	"encoding/json"
	"fmt"
	"strings"
	"time"

	"github.com/charmbracelet/glamour"
	"github.com/charmbracelet/lipgloss"
	"github.com/charmbracelet/x/ansi"
	"github.com/liznear/hh/tools"
	"github.com/liznear/hh/tui/session"
)

const defaultStatusHint = "Enter to send, Shift+Enter newline, PgUp/PgDn scroll, q quit"

type statusWidgetModel struct {
	Busy          bool
	ShowRunResult bool
	SpinnerView   string
	Elapsed       time.Duration
}

func renderStatusWidget(vm statusWidgetModel, theme Theme) string {
	padding := " "
	if vm.Busy {
		spinnerView := lipgloss.NewStyle().Foreground(theme.Info()).Render(vm.SpinnerView)
		durationView := lipgloss.NewStyle().Foreground(theme.Muted()).Render(" " + formatElapsedSeconds(vm.Elapsed))
		return padding + spinnerView + durationView
	}

	if vm.ShowRunResult {
		checkView := lipgloss.NewStyle().Foreground(theme.Success()).Render("✓")
		durationView := lipgloss.NewStyle().Foreground(theme.Muted()).Render(" " + formatElapsedSeconds(vm.Elapsed))
		return padding + checkView + durationView
	}

	return lipgloss.NewStyle().Foreground(theme.Muted()).Render(padding + defaultStatusHint)
}

func formatElapsedSeconds(d time.Duration) string {
	if d < 0 {
		d = 0
	}
	return fmt.Sprintf("%ds", int(d.Truncate(time.Second)/time.Second))
}

func (m *model) renderUserMessageWidget(item *session.UserMessage, width int) []string {
	userLines := wrapLine(item.Content, max(1, width-3))
	if len(userLines) == 0 {
		userLines = []string{""}
	}
	prefix := lipgloss.
		NewStyle().
		Border(lipgloss.NormalBorder(), false).
		BorderLeft(true).
		PaddingLeft(1).
		BorderLeftForeground(m.theme.Accent())
	lines := make([]string, 0, len(userLines))
	for _, line := range userLines {
		lines = append(lines, prefix.Render(line))
	}
	return lines
}

func (m *model) renderAssistantMessageWidget(item *session.AssistantMessage, width int, renderer *glamour.TermRenderer) []string {
	renderedMarkdown, _ := m.renderMarkdown(item.Content, max(1, width-2), renderer)
	assistantLines := strings.Split(renderedMarkdown, "\n")
	for len(assistantLines) > 0 && strings.TrimSpace(ansi.Strip(assistantLines[0])) == "" {
		assistantLines = assistantLines[1:]
	}
	if len(assistantLines) == 0 {
		assistantLines = []string{""}
	}
	lines := make([]string, 0, len(assistantLines))
	for _, line := range assistantLines {
		lines = append(lines, trimOneLeadingSpace(line))
	}
	return lines
}

func (m *model) renderThinkingWidget(item *session.ThinkingBlock, width int, renderer *glamour.TermRenderer) []string {
	renderedMarkdown, _ := m.renderMarkdown(item.Content, max(1, width-2), renderer)
	plainMarkdown := ansi.Strip(renderedMarkdown)
	plainMarkdown = strings.Trim(plainMarkdown, "\r\n")
	thinkingLines := strings.Split(plainMarkdown, "\n")
	for len(thinkingLines) > 0 && strings.TrimSpace(thinkingLines[0]) == "" {
		thinkingLines = thinkingLines[1:]
	}
	if len(thinkingLines) == 0 {
		thinkingLines = []string{""}
	}
	muted := lipgloss.NewStyle().Foreground(m.theme.Muted())
	lines := make([]string, 0, len(thinkingLines))
	for _, line := range thinkingLines {
		line = strings.TrimRight(line, "\r")
		line = strings.TrimLeft(line, " ")
		lines = append(lines, "  "+muted.Render(line))
	}
	return lines
}

func (m *model) renderTurnFooterWidget(modelName string, duration time.Duration, width int) []string {
	bodyWidth := max(1, width-2)
	muted := lipgloss.NewStyle().Foreground(m.theme.Muted())

	meta := strings.TrimSpace(fmt.Sprintf("◆ %s %s", modelName, formatElapsedSeconds(duration)))
	if ansi.StringWidth(meta) >= bodyWidth {
		return []string{"  " + muted.Render(truncateToWidth(meta, bodyWidth))}
	}

	ruleWidth := bodyWidth - ansi.StringWidth(meta) - 1
	rule := strings.Repeat("─", max(0, ruleWidth))
	line := strings.TrimSpace(meta + " " + rule)
	return []string{"  " + muted.Render(line)}
}

func truncateToWidth(s string, maxWidth int) string {
	if maxWidth <= 0 {
		return ""
	}
	if ansi.StringWidth(s) <= maxWidth {
		return s
	}
	if maxWidth == 1 {
		return "…"
	}

	target := maxWidth - 1
	var b strings.Builder
	width := 0
	for _, r := range s {
		rw := ansi.StringWidth(string(r))
		if width+rw > target {
			break
		}
		b.WriteRune(r)
		width += rw
	}
	return b.String() + "…"
}

type styledToken struct {
	raw   string
	style lipgloss.Style
}

type toolCallWidgetModel struct {
	Item  *session.ToolCallItem
	Width int
}

func (m *model) renderToolCallWidget(item *session.ToolCallItem, width int) []string {
	vm := toolCallWidgetModel{Item: item, Width: max(1, width-2)}
	toolLines := renderToolCallWidget(vm, m.theme)
	return prefixedLines(toolLines, "  ")
}

func renderToolCallWidget(vm toolCallWidgetModel, theme Theme) []string {
	if vm.Item == nil {
		return nil
	}

	body, tokens := formatToolCallWidgetBody(vm, theme)
	bodyWidth := max(1, vm.Width-2)
	bodyLines := wrapLine(body, bodyWidth)
	if len(bodyLines) == 0 {
		return []string{renderToolCallIcon(vm, theme)}
	}

	out := make([]string, 0, len(bodyLines))
	for i, line := range bodyLines {
		styledLine := styleToolCallLine(line, tokens)
		if i == 0 {
			out = append(out, fmt.Sprintf("%s %s", renderToolCallIcon(vm, theme), styledLine))
			continue
		}
		out = append(out, "  "+styledLine)
	}

	return out
}

func renderToolCallIcon(vm toolCallWidgetModel, theme Theme) string {
	if vm.Item == nil {
		return ""
	}

	switch vm.Item.Status {
	case session.ToolCallStatusPending:
		return "→"
	case session.ToolCallStatusSuccess:
		return lipgloss.NewStyle().Foreground(theme.Success()).Render("✓")
	default:
		return lipgloss.NewStyle().Foreground(theme.Error()).Render("⨯")
	}
}

func formatToolCallWidgetBody(vm toolCallWidgetModel, theme Theme) (string, []styledToken) {
	item := vm.Item
	if item == nil {
		return "", nil
	}

	args := parseToolCallArgs(item.Arguments)

	pathStyle := lipgloss.NewStyle().Foreground(theme.Info())
	addStyle := lipgloss.NewStyle().Foreground(theme.Success())
	delStyle := lipgloss.NewStyle().Foreground(theme.Error())

	switch strings.ToLower(item.Name) {
	case "list", "ls":
		path := toolArgString(args, "path", ".")
		body := fmt.Sprintf("List %s", path)
		if item.Status == session.ToolCallStatusSuccess {
			if files, ok := listFileCount(item); ok {
				body = fmt.Sprintf("%s (%d files)", body, files)
			}
		}
		return body, []styledToken{{raw: path, style: pathStyle}}

	case "read":
		path := toolArgString(args, "path", ".")
		body := fmt.Sprintf("Read %s", path)
		return body, []styledToken{{raw: path, style: pathStyle}}

	case "grep":
		path := toolArgString(args, "path", ".")
		body := fmt.Sprintf("Grep %s", path)
		if item.Status == session.ToolCallStatusSuccess {
			if matches, ok := grepMatchCount(item); ok {
				body = fmt.Sprintf("%s (%d matches)", body, matches)
			}
		}
		return body, []styledToken{{raw: path, style: pathStyle}}

	case "edit":
		path := toolArgString(args, "path", ".")
		body := fmt.Sprintf("Edit %s", path)
		tokens := []styledToken{{raw: path, style: pathStyle}}
		if item.Status == session.ToolCallStatusSuccess {
			if added, deleted, ok := editCounts(item); ok {
				addToken := fmt.Sprintf("+%d", added)
				delToken := fmt.Sprintf("-%d", deleted)
				body = fmt.Sprintf("%s %s %s", body, addToken, delToken)
				tokens = append(tokens,
					styledToken{raw: addToken, style: addStyle},
					styledToken{raw: delToken, style: delStyle},
				)
			}
		}
		return body, tokens

	default:
		return formatGenericToolCallWidgetBody(item)
	}
}

func styleToolCallLine(line string, tokens []styledToken) string {
	styled := line
	for _, token := range tokens {
		if token.raw == "" {
			continue
		}
		idx := strings.Index(styled, token.raw)
		if idx < 0 {
			continue
		}
		replacement := token.style.Render(token.raw)
		styled = styled[:idx] + replacement + styled[idx+len(token.raw):]
	}
	return styled
}

func parseToolCallArgs(raw string) map[string]any {
	raw = strings.TrimSpace(raw)
	if raw == "" || raw == "{}" {
		return map[string]any{}
	}

	var out map[string]any
	if err := json.Unmarshal([]byte(raw), &out); err != nil {
		return map[string]any{}
	}
	return out
}

func toolArgString(args map[string]any, key string, fallback string) string {
	v, ok := args[key]
	if !ok || v == nil {
		return fallback
	}
	s, ok := v.(string)
	if !ok || strings.TrimSpace(s) == "" {
		return fallback
	}
	return s
}

func listFileCount(item *session.ToolCallItem) (int, bool) {
	if item == nil || item.Result == nil {
		return 0, false
	}
	if result, ok := item.Result.Result.(tools.ListResult); ok {
		return result.FileCount, true
	}
	return 0, false
}

func grepMatchCount(item *session.ToolCallItem) (int, bool) {
	if item == nil || item.Result == nil {
		return 0, false
	}
	if result, ok := item.Result.Result.(tools.GrepResult); ok {
		return result.MatchCount, true
	}
	return 0, false
}

func editCounts(item *session.ToolCallItem) (int, int, bool) {
	if item == nil || item.Result == nil {
		return 0, 0, false
	}
	if result, ok := item.Result.Result.(tools.EditResult); ok {
		return result.AddedLines, result.DeletedLines, true
	}
	return 0, 0, false
}

func formatGenericToolCallWidgetBody(item *session.ToolCallItem) (string, []styledToken) {
	args := strings.TrimSpace(item.Arguments)
	if args == "" || args == "{}" {
		return item.Name, nil
	}

	const maxArgLen = 80
	runes := []rune(args)
	if len(runes) > maxArgLen {
		args = string(runes[:maxArgLen-1]) + "…"
	}

	return fmt.Sprintf("%s %s", item.Name, args), nil
}

func (m *model) renderErrorWidget(item *session.ErrorItem, width int) []string {
	return prefixedLines(wrapLine("error: "+item.Message, max(1, width-2)), "  ")
}

func wrapLine(line string, width int) []string {
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
