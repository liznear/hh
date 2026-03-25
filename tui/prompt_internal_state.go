package tui

import (
	"strings"

	"github.com/liznear/hh/tui/session"
)

func buildInternalState(todoItems []session.TodoItem) string {
	if len(todoItems) == 0 {
		return ""
	}

	b := strings.Builder{}
	b.WriteString("<internal-state>\n")
	b.WriteString("<todo-items>\n")

	for _, item := range todoItems {
		b.WriteString("<todo-item>\n")
		b.WriteString("<content>")
		b.WriteString(xmlEscape(item.Content))
		b.WriteString("</content>\n")
		b.WriteString("<status>")
		b.WriteString(xmlEscape(string(item.Status)))
		b.WriteString("</status>\n")
		b.WriteString("</todo-item>\n")
	}

	b.WriteString("</todo-items>\n")
	b.WriteString("</internal-state>")

	return b.String()
}

func xmlEscape(s string) string {
	replacer := strings.NewReplacer(
		"&", "&amp;",
		"<", "&lt;",
		">", "&gt;",
		`"`, "&quot;",
		"'", "&apos;",
	)
	return replacer.Replace(s)
}
