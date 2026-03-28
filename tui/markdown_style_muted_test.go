package tui

import (
	"strings"
	"testing"

	glamouransi "github.com/charmbracelet/glamour/ansi"
)

func TestMutedStyleConfig_MutesColorFieldsAndPreservesOriginal(t *testing.T) {
	style := glamouransi.StyleConfig{
		Text: glamouransi.StylePrimitive{Color: strPtr("#0000ff")},
		Link: glamouransi.StylePrimitive{Color: strPtr("21")},
		CodeBlock: glamouransi.StyleCodeBlock{
			Chroma: &glamouransi.Chroma{
				Keyword: glamouransi.StylePrimitive{Color: strPtr("#ff0000")},
			},
		},
	}

	muted := mutedStyleConfig(style, 0.6)

	if muted.Text.Color == nil || *muted.Text.Color == "#0000ff" {
		t.Fatalf("expected muted text color, got %v", muted.Text.Color)
	}
	if muted.Link.Color == nil || *muted.Link.Color == "21" {
		t.Fatalf("expected muted link color converted from xterm index, got %v", muted.Link.Color)
	}
	if muted.CodeBlock.Chroma == nil || muted.CodeBlock.Chroma.Keyword.Color == nil || *muted.CodeBlock.Chroma.Keyword.Color == "#ff0000" {
		t.Fatalf("expected muted nested chroma color, got %+v", muted.CodeBlock.Chroma)
	}

	// Original input must remain unchanged.
	if style.Text.Color == nil || *style.Text.Color != "#0000ff" {
		t.Fatalf("expected original text color preserved, got %v", style.Text.Color)
	}
	if style.Link.Color == nil || *style.Link.Color != "21" {
		t.Fatalf("expected original link color preserved, got %v", style.Link.Color)
	}
	if style.CodeBlock.Chroma == nil || style.CodeBlock.Chroma.Keyword.Color == nil || *style.CodeBlock.Chroma.Keyword.Color != "#ff0000" {
		t.Fatalf("expected original nested chroma color preserved, got %+v", style.CodeBlock.Chroma)
	}
}

func TestMutedStyleConfig_BlendsTowardBackground(t *testing.T) {
	style := glamouransi.StyleConfig{
		Document: glamouransi.StyleBlock{StylePrimitive: glamouransi.StylePrimitive{BackgroundColor: strPtr("#ffffff")}},
		Text:     glamouransi.StylePrimitive{Color: strPtr("#000000")},
	}

	muted := mutedStyleConfig(style, 0.2)
	if muted.Text.Color == nil {
		t.Fatal("expected muted text color")
	}
	if got := strings.ToLower(*muted.Text.Color); got != "#333333" {
		t.Fatalf("expected blended color #333333, got %s", got)
	}
}

func TestMutedStyleConfig_LeavesUnsupportedColorStringsUntouched(t *testing.T) {
	style := glamouransi.StyleConfig{
		Text: glamouransi.StylePrimitive{Color: strPtr("blue")},
	}

	muted := mutedStyleConfig(style, 0.5)
	if muted.Text.Color == nil || *muted.Text.Color != "blue" {
		t.Fatalf("expected unsupported color string unchanged, got %v", muted.Text.Color)
	}
}

func TestMutedStyleConfig_ZeroAmountNoChanges(t *testing.T) {
	style := glamouransi.StyleConfig{
		Text: glamouransi.StylePrimitive{Color: strPtr("#112233")},
	}

	muted := mutedStyleConfig(style, 0)
	if muted.Text.Color == nil || *muted.Text.Color != "#112233" {
		t.Fatalf("expected no changes with zero amount, got %v", muted.Text.Color)
	}
}

func strPtr(s string) *string { return &s }
