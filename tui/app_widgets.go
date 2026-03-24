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

type statusWidgetModel struct {
	AgentName     string
	ModelName     string
	Busy          bool
	ShowRunResult bool
	SpinnerView   string
	Elapsed       time.Duration
	EscPending    bool
	ShellMode     bool
}

func renderStatusWidget(vm statusWidgetModel, theme Theme) string {
	padding := "  "
	if vm.ShellMode && !vm.Busy {
		return padding + "Shell"
	}
	base := strings.TrimSpace(vm.AgentName)
	if base == "" {
		base = "Build"
	}
	if strings.TrimSpace(vm.ModelName) != "" {
		base = fmt.Sprintf("%s · %s", base, vm.ModelName)
	}

	if vm.Busy {
		spinnerView := lipgloss.NewStyle().Foreground(theme.Color(ThemeColorStatusSpinnerForeground)).Render(vm.SpinnerView)
		durationView := lipgloss.NewStyle().Foreground(theme.Color(ThemeColorStatusDurationForeground)).Render(formatElapsedSeconds(vm.Elapsed))
		hint := ""
		if vm.EscPending {
			hint = lipgloss.NewStyle().Foreground(theme.Color(ThemeColorStatusInterruptHintForeground)).Render(" esc again to interrupt")
		}
		return fmt.Sprintf("%s%s · %s %s%s", padding, base, durationView, spinnerView, hint)
	}

	if vm.ShowRunResult {
		durationView := lipgloss.NewStyle().Foreground(theme.Color(ThemeColorStatusDurationForeground)).Render(formatElapsedSeconds(vm.Elapsed))
		return fmt.Sprintf("%s%s · %s", padding, base, durationView)
	}

	return padding + base
}

func formatElapsedSeconds(d time.Duration) string {
	if d < 0 {
		d = 0
	}
	return fmt.Sprintf("%ds", int(d.Truncate(time.Second)/time.Second))
}

func (m *model) renderUserMessageWidget(item *session.UserMessage, width int) []string {
	content := item.Content
	if item != nil && item.Queued {
		badge := lipgloss.NewStyle().
			Foreground(m.theme.Background()).
			Background(m.theme.Foreground()).
			Padding(0, 1).
			Render("Queued")
		content = badge + " " + content
	}
	userLines := wrapLine(content, max(1, width-3))
	if len(userLines) == 0 {
		userLines = []string{""}
	}
	prefix := lipgloss.
		NewStyle().
		Border(lipgloss.NormalBorder(), false).
		BorderLeft(true).
		PaddingLeft(1).
		BorderLeftForeground(m.theme.Color(ThemeColorUserMessageBorderForeground))
	lines := make([]string, 0, len(userLines))
	for _, line := range userLines {
		lines = append(lines, prefix.Render(line))
	}
	return lines
}

func (m *model) renderShellMessageWidget(item *session.ShellMessage, width int) []string {
	if item == nil {
		return []string{""}
	}

	const shellBoxLeftMargin = 2
	boxWidth := max(1, width-2-shellBoxLeftMargin)
	innerWidth := max(1, boxWidth-2)

	contentLines := []string{"$ " + item.Command, ""}
	output := item.Output
	if output == "" {
		output = "(no output)"
	}
	for _, line := range strings.Split(output, "\n") {
		wrapped := wrapLine(line, innerWidth)
		if len(wrapped) == 0 {
			contentLines = append(contentLines, "")
			continue
		}
		contentLines = append(contentLines, wrapped...)
	}

	box := lipgloss.NewStyle().
		Background(m.theme.Color(ThemeColorShellMessageBackground)).
		Padding(1).
		MarginLeft(shellBoxLeftMargin).
		Width(boxWidth).
		Render(strings.Join(contentLines, "\n"))

	return strings.Split(box, "\n")
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
	muted := lipgloss.NewStyle().Foreground(m.theme.Color(ThemeColorThinkingForeground))
	lines := make([]string, 0, len(thinkingLines))
	for _, line := range thinkingLines {
		line = strings.TrimRight(line, "\r")
		line = strings.TrimLeft(line, " ")
		lines = append(lines, "  "+muted.Render(line))
	}
	return lines
}

func (m *model) renderTurnFooterWidget(modelName string, duration time.Duration, status string, width int) []string {
	bodyWidth := max(1, width-2)
	muted := lipgloss.NewStyle().Foreground(m.theme.Color(ThemeColorTurnFooterForeground))

	statusLabel := ""
	if strings.EqualFold(status, "cancelled") {
		statusLabel = " Cancelled"
	}
	meta := strings.TrimSpace(fmt.Sprintf("◆ %s %s%s", modelName, formatElapsedSeconds(duration), statusLabel))
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
	Item       *session.ToolCallItem
	Width      int
	WorkingDir string
}

func (m *model) renderToolCallWidget(item *session.ToolCallItem, width int) []string {
	vm := toolCallWidgetModel{Item: item, Width: max(1, width-2), WorkingDir: m.workingDir}
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
		return lipgloss.NewStyle().Foreground(theme.Color(ThemeColorToolCallIconSuccessForeground)).Render("✓")
	default:
		return lipgloss.NewStyle().Foreground(theme.Color(ThemeColorToolCallIconErrorForeground)).Render("⨯")
	}
}

func formatToolCallWidgetBody(vm toolCallWidgetModel, theme Theme) (string, []styledToken) {
	item := vm.Item
	if item == nil {
		return "", nil
	}

	args := parseToolCallArgs(item.Arguments)

	pathStyle := lipgloss.NewStyle().Foreground(theme.Color(ThemeColorToolCallPathForeground))
	addStyle := lipgloss.NewStyle().Foreground(theme.Color(ThemeColorToolCallAddForeground))
	delStyle := lipgloss.NewStyle().Foreground(theme.Color(ThemeColorToolCallDeleteForeground))

	switch strings.ToLower(item.Name) {
	case "list", "ls":
		path := beautifyToolPath(toolArgString(args, "path", "."), vm.WorkingDir)
		body := fmt.Sprintf("List %s", path)
		if item.Status == session.ToolCallStatusSuccess {
			if files, ok := listFileCount(item); ok {
				body = fmt.Sprintf("%s (%d files)", body, files)
			}
		}
		return body, []styledToken{{raw: path, style: pathStyle}}

	case "read":
		path := beautifyToolPath(toolArgString(args, "path", "."), vm.WorkingDir)
		body := fmt.Sprintf("Read %s", path)
		return body, []styledToken{{raw: path, style: pathStyle}}

	case "grep":
		path := beautifyToolPath(toolArgString(args, "path", "."), vm.WorkingDir)
		body := fmt.Sprintf("Grep %s", path)
		if item.Status == session.ToolCallStatusSuccess {
			if matches, ok := grepMatchCount(item); ok {
				body = fmt.Sprintf("%s (%d matches)", body, matches)
			}
		}
		return body, []styledToken{{raw: path, style: pathStyle}}

	case "edit":
		path := beautifyToolPath(toolArgString(args, "path", "."), vm.WorkingDir)
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

	case "write":
		path := beautifyToolPath(toolArgString(args, "path", "."), vm.WorkingDir)
		body := fmt.Sprintf("Write %s", path)
		tokens := []styledToken{{raw: path, style: pathStyle}}
		if item.Status == session.ToolCallStatusSuccess {
			if added, ok := writeAddedLines(item); ok {
				addToken := fmt.Sprintf("+%d", added)
				body = fmt.Sprintf("%s %s", body, addToken)
				tokens = append(tokens, styledToken{raw: addToken, style: addStyle})
			}
		}
		return body, tokens

	case "web_search":
		query := toolArgString(args, "query", "")
		body := fmt.Sprintf("WebSearch %q", query)
		return body, []styledToken{{raw: query, style: pathStyle}}

	case "web_fetch":
		url := toolArgString(args, "url", "")
		body := fmt.Sprintf("WebFetch %q", url)
		return body, []styledToken{{raw: url, style: pathStyle}}

	case "glob":
		pattern := toolArgString(args, "pattern", "*")
		path := beautifyToolPath(toolArgString(args, "path", "."), vm.WorkingDir)
		body := fmt.Sprintf("Glob %q in %s", pattern, path)
		tokens := []styledToken{{raw: pattern, style: pathStyle}, {raw: path, style: pathStyle}}
		if item.Status == session.ToolCallStatusSuccess {
			if matches, ok := globMatchCount(item); ok {
				body = fmt.Sprintf("%s (%d matches)", body, matches)
			}
		}
		return body, tokens

	case "bash":
		command := toolArgString(args, "command", "")
		if command == "" {
			return "Bash", nil
		}
		commandMaxLen := min(50, vm.Width-20)
		displayCommand := truncateToolCommand(command, commandMaxLen)
		body := fmt.Sprintf("Bash %q", displayCommand)
		return body, []styledToken{{raw: displayCommand, style: pathStyle}}

	case "todo_write":
		done, total, ok := todoProgress(item, args)
		if ok {
			return fmt.Sprintf("TODO %d / %d", done, total), nil
		}
		return "TODO", nil

	case "question":
		title := questionTitleArg(args)
		if title == "" {
			return `Question: ""`, nil
		}
		return fmt.Sprintf("Question: %q", title), []styledToken{{raw: title, style: pathStyle}}

	case "skill":
		skillName := toolArgString(args, "name", "")
		if skillName == "" && item.Result != nil {
			if result, ok := item.Result.Result.(tools.SkillResult); ok {
				skillName = result.Name
			}
		}
		if skillName == "" {
			return "Skill", nil
		}
		return fmt.Sprintf("Skill %q", skillName), []styledToken{{raw: skillName, style: pathStyle}}

	default:
		return formatGenericToolCallWidgetBody(item)
	}
}

func questionTitleArg(args map[string]any) string {
	rawQuestion, ok := args["question"]
	if !ok || rawQuestion == nil {
		return ""
	}
	question, ok := rawQuestion.(map[string]any)
	if !ok {
		return ""
	}
	title, ok := question["title"].(string)
	if !ok {
		return ""
	}
	return strings.TrimSpace(title)
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

func globMatchCount(item *session.ToolCallItem) (int, bool) {
	if item == nil || item.Result == nil {
		return 0, false
	}
	if result, ok := item.Result.Result.(tools.GlobResult); ok {
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

func writeAddedLines(item *session.ToolCallItem) (int, bool) {
	if item == nil || item.Result == nil {
		return 0, false
	}
	if result, ok := item.Result.Result.(tools.WriteResult); ok {
		return result.AddedLines, true
	}
	return 0, false
}

func todoProgress(item *session.ToolCallItem, args map[string]any) (int, int, bool) {
	if done, total, ok := todoProgressFromResult(item); ok {
		return done, total, true
	}
	return todoProgressFromArgs(args)
}

func todoProgressFromResult(item *session.ToolCallItem) (int, int, bool) {
	if item == nil || item.Result == nil {
		return 0, 0, false
	}

	var todoItems []tools.TodoItem
	switch result := item.Result.Result.(type) {
	case tools.TodoWriteResult:
		todoItems = result.TodoItems
	case *tools.TodoWriteResult:
		if result == nil {
			return 0, 0, false
		}
		todoItems = result.TodoItems
	default:
		return 0, 0, false
	}

	done := 0
	for _, todo := range todoItems {
		status := strings.ToLower(strings.TrimSpace(string(todo.Status)))
		if status == string(session.TodoStatusCompleted) || status == string(session.TodoStatusCancelled) {
			done++
		}
	}
	return done, len(todoItems), true
}

func todoProgressFromArgs(args map[string]any) (int, int, bool) {
	raw, ok := args["todo_items"]
	if !ok || raw == nil {
		return 0, 0, false
	}

	todoItems, ok := raw.([]any)
	if !ok {
		return 0, 0, false
	}

	done := 0
	for _, rawItem := range todoItems {
		itemMap, ok := rawItem.(map[string]any)
		if !ok {
			continue
		}
		status, ok := itemMap["status"].(string)
		if !ok {
			continue
		}
		normalized := strings.ToLower(strings.TrimSpace(status))
		if normalized == string(session.TodoStatusCompleted) || normalized == string(session.TodoStatusCancelled) {
			done++
		}
	}

	return done, len(todoItems), true
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

func truncateToolCommand(command string, maxLen int) string {
	if maxLen <= 0 {
		return ""
	}

	runes := []rune(command)
	if len(runes) <= maxLen {
		return command
	}

	if maxLen <= 3 {
		return strings.Repeat(".", maxLen)
	}

	return string(runes[:maxLen-3]) + "..."
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
