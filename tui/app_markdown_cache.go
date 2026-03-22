package tui

import (
	"fmt"
	"reflect"
	"strings"

	"github.com/charmbracelet/glamour"
	"github.com/liznear/hh/tui/session"
)

func (m *model) getCachedRenderedItem(item session.Item, width int) ([]string, bool) {
	if m.itemRenderCache == nil {
		return nil, false
	}
	if item == nil {
		return nil, false
	}
	if tc, ok := item.(*session.ToolCallItem); ok && tc.Status == session.ToolCallStatusPending {
		return nil, false
	}
	key := itemCacheKey(item)
	if key == 0 {
		return nil, false
	}
	entry, ok := m.itemRenderCache[key]
	if !ok {
		return nil, false
	}
	sig, ok := itemCacheSignature(item)
	if !ok || entry.width != width || entry.signature != sig {
		return nil, false
	}
	return cloneStringSlice(entry.lines), true
}

func (m *model) setCachedRenderedItem(item session.Item, width int, lines []string) {
	if m.itemRenderCache == nil {
		return
	}
	if item == nil {
		return
	}
	if tc, ok := item.(*session.ToolCallItem); ok && tc.Status == session.ToolCallStatusPending {
		return
	}
	key := itemCacheKey(item)
	if key == 0 {
		return
	}
	sig, ok := itemCacheSignature(item)
	if !ok {
		return
	}
	m.itemRenderCache[key] = itemRenderCacheEntry{
		width:     width,
		signature: sig,
		lines:     cloneStringSlice(lines),
	}
}

func itemCacheKey(item session.Item) uintptr {
	v := reflect.ValueOf(item)
	if !v.IsValid() || v.Kind() != reflect.Ptr || v.IsNil() {
		return 0
	}
	return v.Pointer()
}

func itemCacheSignature(item session.Item) (string, bool) {
	switch v := item.(type) {
	case *session.UserMessage:
		return "user:" + v.Content, true
	case *session.AssistantMessage:
		return "assistant:" + v.Content, true
	case *session.ThinkingBlock:
		return "thinking:" + v.Content, true
	case *session.ToolCallItem:
		summary := ""
		if v.Result != nil {
			summary = v.ResultSummary()
		}
		return fmt.Sprintf("tool:%d:%s:%s:%s:%t", v.Status, v.Name, v.Arguments, summary, v.Result != nil), true
	case *session.ErrorItem:
		return "error:" + v.Message, true
	default:
		return "", false
	}
}

func cloneStringSlice(in []string) []string {
	out := make([]string, len(in))
	copy(out, in)
	return out
}

func (m *model) getMarkdownRenderer(width int) *glamour.TermRenderer {
	if m.markdownRenderer != nil && m.markdownRendererWidth == width {
		return m.markdownRenderer
	}

	renderer, err := glamour.NewTermRenderer(
		glamour.WithStandardStyle("light"),
		glamour.WithPreservedNewLines(),
		glamour.WithWordWrap(max(20, width)),
	)
	if err != nil {
		m.markdownRenderer = nil
		m.markdownRendererWidth = 0
		return nil
	}

	m.markdownRenderer = renderer
	m.markdownRendererWidth = width
	m.markdownCache = map[string]string{}
	return m.markdownRenderer
}

func (m *model) renderMarkdown(content string, width int, renderer *glamour.TermRenderer) (string, markdownPerfStats) {
	stats := markdownPerfStats{}
	if strings.TrimSpace(content) == "" {
		return "", stats
	}

	cacheKey := fmt.Sprintf("%d:%s", width, content)
	if cached, ok := m.markdownCache[cacheKey]; ok {
		return cached, stats
	}

	if renderer == nil {
		fallback := strings.Join(wrapLine(content, width), "\n")
		m.markdownCache[cacheKey] = fallback
		return fallback, stats
	}

	rendered, err := renderer.Render(content)
	if err != nil {
		fallback := strings.Join(wrapLine(content, width), "\n")
		m.markdownCache[cacheKey] = fallback
		return fallback, stats
	}

	trimmed := strings.TrimRight(rendered, "\n")
	m.markdownCache[cacheKey] = trimmed
	return trimmed, stats
}
