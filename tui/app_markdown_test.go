package tui

import (
	"strings"
	"testing"
)

func TestRenderMarkdown_DoesNotFallbackForModeratelyLargeMarkdown(t *testing.T) {
	m := &model{}
	renderer := m.getMarkdownRenderer(80)
	if renderer == nil {
		t.Fatal("expected markdown renderer")
	}

	content := strings.Repeat("- `item`\n", 3000)
	rendered, stats := m.renderMarkdown(content, 80, renderer)

	if stats.fallbackToWrap {
		t.Fatal("expected markdown render without fallback")
	}
	if strings.Contains(rendered, "`item`") {
		t.Fatal("expected inline markdown to be rendered, got raw markdown markers")
	}
}

func TestRenderMarkdown_FallsBackWhenRendererUnavailable(t *testing.T) {
	m := &model{}
	content := strings.Repeat("- `item`\n", 100)
	rendered, stats := m.renderMarkdown(content, 80, nil)

	if stats.fallbackToWrap {
		t.Fatal("expected renderer-unavailable fallback without budget fallback flag")
	}
	if !strings.Contains(rendered, "`item`") {
		t.Fatal("expected fallback output to preserve raw markdown")
	}
}
