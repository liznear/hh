package tui

import (
	"strings"
	"time"

	"github.com/liznear/hh/tui/session"
)

func buildInternalState(todoItems []session.TodoItem, mentionedFiles []mentionedFileContent) string {
	timestamp := time.Now().UTC().Format(time.RFC3339)

	b := strings.Builder{}
	b.WriteString("<internal-state>\n")
	b.WriteString("<timestamp>")
	b.WriteString(xmlEscape(timestamp))
	b.WriteString("</timestamp>\n")

	if len(todoItems) > 0 {
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
	}

	for _, file := range mentionedFiles {
		if strings.TrimSpace(file.Path) == "" {
			continue
		}
		b.WriteString("<file-contents@")
		b.WriteString(xmlEscape(file.Path))
		b.WriteString(">\n")
		b.WriteString(xmlEscape(file.Content))
		b.WriteString("\n</file-contents@")
		b.WriteString(xmlEscape(file.Path))
		b.WriteString(">\n")
	}

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
