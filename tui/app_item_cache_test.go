package tui

import (
	"strings"
	"testing"

	"github.com/liznear/hh/tui/session"
)

func TestFormatSessionForViewport_ReusesAndUpdatesItemRenderCache(t *testing.T) {
	m := &model{
		session:         session.NewState("test-model"),
		markdownCache:   map[string]string{},
		itemRenderCache: map[uintptr]itemRenderCacheEntry{},
	}

	msg := &session.UserMessage{Content: "hello"}
	m.session.AddItem(msg)

	first := m.renderMessageList(30, 5)
	if !strings.Contains(first, "hello") {
		t.Fatalf("expected first render to include initial message, got %q", first)
	}
	if got := len(m.itemRenderCache); got != 1 {
		t.Fatalf("expected 1 cached item after first render, got %d", got)
	}

	second := m.renderMessageList(30, 5)
	if second != first {
		t.Fatalf("expected second render to match first render, got %q vs %q", second, first)
	}
	if got := len(m.itemRenderCache); got != 1 {
		t.Fatalf("expected cache size to stay 1, got %d", got)
	}

	msg.Content = "updated"
	third := m.renderMessageList(30, 5)
	if !strings.Contains(third, "updated") {
		t.Fatalf("expected updated render content, got %q", third)
	}
	if strings.Contains(third, "hello") {
		t.Fatalf("expected old content to be invalidated, got %q", third)
	}
	if got := len(m.itemRenderCache); got != 1 {
		t.Fatalf("expected cache size to stay 1 after update, got %d", got)
	}
}
