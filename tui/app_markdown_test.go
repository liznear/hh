package tui

import (
	"regexp"
	"strings"
	"testing"

	"github.com/charmbracelet/glamour"
)

func TestRenderMarkdown_RendersModeratelyLargeMarkdown(t *testing.T) {
	content := strings.Repeat("- `item`\n", 3000)
	rendered := RenderMarkdown(content, 80)

	if strings.Contains(rendered, "`item`") {
		t.Fatal("expected inline markdown to be rendered, got raw markdown markers")
	}
}

func TestRenderMarkdown_EmptyContent(t *testing.T) {
	rendered := RenderMarkdown("   \n\t", 80)

	if rendered != "" {
		t.Fatal("expected empty markdown content to render as empty string")
	}
}

func TestRenderMarkdownThinking_MutesCodeBlockSyntaxColors(t *testing.T) {
	content := "```go\nfunc main() { return }\n```"
	rendered := RenderMarkdown(content, 80, ThinkingOption())

	if strings.Contains(rendered, "\x1b[38;5;39m") {
		t.Fatalf("expected thinking markdown code block colors to be muted, got %q", rendered)
	}
}

func TestRenderMarkdownThinking_PreservesSyntaxHighlightingInCodeBlock(t *testing.T) {
	content := "```go\nfunc main() { return }\n```"
	rendered := RenderMarkdown(content, 80, ThinkingOption())

	re := regexp.MustCompile(`\x1b\[38;5;(\d+)m`)
	matches := re.FindAllStringSubmatch(rendered, -1)
	seen := map[string]struct{}{}
	for _, m := range matches {
		if len(m) != 2 {
			continue
		}
		seen[m[1]] = struct{}{}
	}
	if len(seen) < 3 {
		t.Fatalf("expected multiple muted syntax colors in code block, got %q", rendered)
	}
}

func TestThinkingOption_ReducesDefaultStyleColors(t *testing.T) {
	original := *glamour.DefaultStyles["light"]
	style := original
	opt := ThinkingOption()
	opt.apply(&style)

	if style.Document.Color == nil || original.Document.Color == nil || *style.Document.Color == *original.Document.Color {
		t.Fatalf("expected thinking option to reduce document color; before=%v after=%v", original.Document.Color, style.Document.Color)
	}
	if style.Code.Color == nil || original.Code.Color == nil || *style.Code.Color == *original.Code.Color {
		t.Fatalf("expected thinking option to reduce inline code color; before=%v after=%v", original.Code.Color, style.Code.Color)
	}
	if style.CodeBlock.Theme != thinkingCodeBlockThemeName {
		t.Fatalf("expected thinking option to use custom muted code block theme %q, got %q", thinkingCodeBlockThemeName, style.CodeBlock.Theme)
	}
	if style.CodeBlock.Chroma != nil {
		t.Fatal("expected thinking option to clear inline chroma config to force theme-based muted syntax highlighting")
	}
	if style.Document.Margin == nil || *style.Document.Margin != 2 {
		t.Fatalf("expected thinking option to set document margin to 2, got %v", style.Document.Margin)
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
