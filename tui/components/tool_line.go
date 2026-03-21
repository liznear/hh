package components

import (
	"encoding/json"
	"fmt"
	"strings"

	"github.com/charmbracelet/lipgloss"
	"github.com/liznear/hh/tools"
	"github.com/liznear/hh/tui/session"
)

type styledToken struct {
	raw   string
	style lipgloss.Style
}

func RenderToolCall(item *session.ToolCallItem, width int, successColor lipgloss.Color, errorColor lipgloss.Color, pathColor lipgloss.Color, addedColor lipgloss.Color, deletedColor lipgloss.Color) []string {
	if item == nil {
		return nil
	}

	params := renderToolCallParams{
		Item:         item,
		Width:        width,
		SuccessColor: successColor,
		ErrorColor:   errorColor,
		PathColor:    pathColor,
		AddedColor:   addedColor,
		DeletedColor: deletedColor,
	}

	body, tokens := formatToolCallBody(params)
	bodyWidth := max(1, params.Width-2)
	bodyLines := WrapLine(body, bodyWidth)
	if len(bodyLines) == 0 {
		return []string{renderToolCallIcon(params)}
	}

	out := make([]string, 0, len(bodyLines))
	for i, line := range bodyLines {
		styledLine := styleToolCallLine(line, tokens)
		if i == 0 {
			out = append(out, fmt.Sprintf("%s %s", renderToolCallIcon(params), styledLine))
			continue
		}
		out = append(out, "  "+styledLine)
	}

	return out
}

type renderToolCallParams struct {
	Item  *session.ToolCallItem
	Width int

	SuccessColor lipgloss.Color
	ErrorColor   lipgloss.Color
	PathColor    lipgloss.Color
	AddedColor   lipgloss.Color
	DeletedColor lipgloss.Color
}

func renderToolCallIcon(params renderToolCallParams) string {
	if params.Item == nil {
		return ""
	}

	switch params.Item.Status {
	case session.ToolCallStatusPending:
		return "→"
	case session.ToolCallStatusSuccess:
		return lipgloss.NewStyle().Foreground(params.SuccessColor).Render("✓")
	default:
		return lipgloss.NewStyle().Foreground(params.ErrorColor).Render("⨯")
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

func formatToolCallBody(params renderToolCallParams) (string, []styledToken) {
	item := params.Item
	if item == nil {
		return "", nil
	}

	args := parseToolCallArgs(item.Arguments)

	pathStyle := lipgloss.NewStyle().Foreground(params.PathColor)
	addStyle := lipgloss.NewStyle().Foreground(params.AddedColor)
	delStyle := lipgloss.NewStyle().Foreground(params.DeletedColor)

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
		return formatGenericToolCallBody(item)
	}
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

func formatGenericToolCallBody(item *session.ToolCallItem) (string, []styledToken) {
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
