package tui

import (
	"strings"
	"testing"

	"github.com/charmbracelet/x/ansi"
	"github.com/liznear/hh/tui/session"
)

func TestRenderMessageList_MixedContentRespectsWidthAndHeight(t *testing.T) {
	m := &model{
		theme:           DefaultTheme(),
		session:         session.NewState("test-model"),
		markdownCache:   map[string]string{},
		itemRenderCache: map[uintptr]itemRenderCacheEntry{},
	}

	m.session.AddItem(&session.UserMessage{Content: "plain " + strings.Repeat("text ", 20)})
	m.session.AddItem(&session.AssistantMessage{Content: "Markdown:\n\n```go\n" + strings.Repeat("x", 140) + "\n```"})
	m.session.AddItem(&session.UserMessage{Content: "tail"})

	const width = 40
	const height = 8

	assertRenderedFrameFits(t, m.renderMessageList(width, height), width, height)

	// Exercise an offset where markdown + non-markdown lines are both visible.
	m.listOffsetIdx = 1
	m.listOffsetLine = 1
	assertRenderedFrameFits(t, m.renderMessageList(width, height), width, height)
}

func assertRenderedFrameFits(t *testing.T, frame string, width int, height int) {
	t.Helper()

	lines := strings.Split(frame, "\n")
	if len(lines) != height {
		t.Fatalf("expected %d lines, got %d", height, len(lines))
	}

	for i, line := range lines {
		if got := ansi.StringWidth(line); got > width {
			t.Fatalf("line %d exceeds width: got %d, max %d; line=%q", i, got, width, line)
		}
	}
}
