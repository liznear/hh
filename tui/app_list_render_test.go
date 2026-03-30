package tui

import (
	"strings"
	"testing"

	"github.com/charmbracelet/x/ansi"
	"github.com/liznear/hh/tui/session"
)

func TestRenderMessageList_MixedContentRespectsWidthAndHeight(t *testing.T) {
	m := newTestModel()

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

func TestRenderMessageList_InsertsSingleBlankLineBetweenMessageBlocks(t *testing.T) {
	m := newTestModel()

	m.session.AddItem(&session.UserMessage{Content: "user message"})
	m.session.AddItem(&session.ThinkingBlock{Content: "thinking message"})
	m.session.AddItem(&session.ToolCallItem{Name: "read", Arguments: `{"path":"a.txt"}`})
	m.session.AddItem(&session.ToolCallItem{Name: "list", Arguments: `{"path":"."}`})
	m.session.AddItem(&session.AssistantMessage{Content: "assistant message"})

	frame := ansi.Strip(m.renderMessageList(120, 40))
	lines := strings.Split(frame, "\n")

	userIdx := lineIndexContaining(lines, "user message")
	thinkingIdx := lineIndexContaining(lines, "thinking message")
	readIdx := lineIndexContaining(lines, "Read a.txt [start=0, limit=0]")
	listIdx := lineIndexContaining(lines, "List .")
	assistantIdx := lineIndexContaining(lines, "assistant message")

	if userIdx < 0 || thinkingIdx < 0 || readIdx < 0 || listIdx < 0 || assistantIdx < 0 {
		t.Fatalf("missing expected rendered content in frame: %q", frame)
	}

	if thinkingIdx != userIdx+2 {
		t.Fatalf("expected one blank line between user and thinking blocks, got user=%d thinking=%d", userIdx, thinkingIdx)
	}
	if readIdx != thinkingIdx+2 {
		t.Fatalf("expected one blank line between thinking and tool blocks, got thinking=%d read=%d", thinkingIdx, readIdx)
	}
	if listIdx != readIdx+1 {
		t.Fatalf("expected no blank line between consecutive tool lines, got read=%d list=%d", readIdx, listIdx)
	}
	if assistantIdx != listIdx+2 {
		t.Fatalf("expected one blank line between tool and assistant blocks, got list=%d assistant=%d", listIdx, assistantIdx)
	}
}

func TestRenderMessageList_ShowsMutedTurnFooterAfterAssistantOnTurnEnd(t *testing.T) {
	m := newTestModel()

	turn := m.session.StartTurn()
	turn.AddItem(&session.AssistantMessage{Content: "assistant message"})
	turn.End()

	frame := ansi.Strip(m.renderMessageList(120, 40))
	lines := strings.Split(frame, "\n")

	assistantIdx := lineIndexContaining(lines, "assistant message")
	footerIdx := lineIndexContaining(lines, "◆ Build · test-model 0s")

	if assistantIdx < 0 || footerIdx < 0 {
		t.Fatalf("missing assistant or footer in frame: %q", frame)
	}
	if footerIdx <= assistantIdx {
		t.Fatalf("expected footer after assistant message, got assistant=%d footer=%d", assistantIdx, footerIdx)
	}
	if footerIdx != assistantIdx+2 {
		t.Fatalf("expected exactly one blank line before footer, got assistant=%d footer=%d", assistantIdx, footerIdx)
	}
	if !strings.Contains(lines[footerIdx], "◆ Build · test-model 0s") || !strings.Contains(lines[footerIdx], "─") {
		t.Fatalf("expected footer metadata and separator on same line, got %q", lines[footerIdx])
	}
}

func TestRenderMessageList_ShowsCancelledTurnFooter(t *testing.T) {
	m := newTestModel()

	turn := m.session.StartTurn()
	turn.AddItem(&session.AssistantMessage{Content: "assistant message"})
	turn.EndWithStatus("cancelled")

	frame := ansi.Strip(m.renderMessageList(120, 40))
	lines := strings.Split(frame, "\n")

	footerIdx := lineIndexContaining(lines, "◆ Build · test-model 0s Cancelled")
	if footerIdx < 0 {
		t.Fatalf("missing cancelled footer in frame: %q", frame)
	}
	if !strings.Contains(lines[footerIdx], "◆ Build · test-model 0s Cancelled") || !strings.Contains(lines[footerIdx], "─") {
		t.Fatalf("expected cancelled footer metadata and separator on same line, got %q", lines[footerIdx])
	}
}

func TestRenderMessageList_ShowsCompactionMarkerSeparator(t *testing.T) {
	m := newTestModel()

	turn := m.session.StartTurn()
	turn.AddItem(&session.CompactionMarker{})

	frame := ansi.Strip(m.renderMessageList(80, 10))
	if !strings.Contains(frame, "Compaction") {
		t.Fatalf("expected compaction marker in frame, got %q", frame)
	}
	if !strings.Contains(frame, "─") {
		t.Fatalf("expected separator rule in compaction marker, got %q", frame)
	}
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

func lineIndexContaining(lines []string, needle string) int {
	for i, line := range lines {
		if strings.Contains(line, needle) {
			return i
		}
	}
	return -1
}
