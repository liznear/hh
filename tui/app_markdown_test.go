package tui

import (
	"strings"
	"testing"
)

func TestRenderMarkdown_RendersModeratelyLargeMarkdown(t *testing.T) {
	content := strings.Repeat("- `item`\n", 3000)
	rendered := renderMarkdown(content, 80)

	if strings.Contains(rendered, "`item`") {
		t.Fatal("expected inline markdown to be rendered, got raw markdown markers")
	}
}

func TestRenderMarkdown_EmptyContent(t *testing.T) {
	rendered := renderMarkdown("   \n\t", 80)

	if rendered != "" {
		t.Fatal("expected empty markdown content to render as empty string")
	}
}

func TestGetMarkdownRenderer_CachesByWidthAndOptionName(t *testing.T) {
	r80 := getMarkdownRenderer(80)
	r100 := getMarkdownRenderer(100)
	r80Again := getMarkdownRenderer(80)
	r80Thinking := getMarkdownRenderer(80, ThinkingOption())
	r80ThinkingAgain := getMarkdownRenderer(80, ThinkingOption())

	if r80 == nil || r100 == nil || r80Again == nil || r80Thinking == nil || r80ThinkingAgain == nil {
		t.Fatal("expected markdown renderers for all widths/options")
	}
	if r80 == r100 {
		t.Fatal("expected different widths to use different renderer instances")
	}
	if r80Again != r80 {
		t.Fatal("expected same width+options to reuse cached renderer instance")
	}
	if r80Thinking == r80 {
		t.Fatal("expected distinct option names to use distinct renderer cache entries")
	}
	if r80ThinkingAgain != r80Thinking {
		t.Fatal("expected same option name to reuse cached renderer instance")
	}
}
