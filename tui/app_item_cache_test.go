package tui

import (
	"strings"
	"testing"

	"github.com/liznear/hh/tui/session"
)

func TestFormatSessionForViewport_ReusesAndUpdatesItemRenderCache(t *testing.T) {
	m := newTestModel()

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

func TestItemCacheSignature_UserMessageIncludesQueuedFlag(t *testing.T) {
	queuedSig, ok := itemCacheSignature(&session.UserMessage{Content: "same", Queued: true})
	if !ok {
		t.Fatal("expected queued user message signature")
	}

	normalSig, ok := itemCacheSignature(&session.UserMessage{Content: "same", Queued: false})
	if !ok {
		t.Fatal("expected normal user message signature")
	}

	if queuedSig == normalSig {
		t.Fatalf("expected different signatures for queued vs non-queued user messages, got %q", queuedSig)
	}
}

func TestSetCachedRenderedItem_DoesNotCacheQueuedUserMessage(t *testing.T) {
	m := newTestModel()

	m.setCachedRenderedItem(&session.UserMessage{Content: "steer", Queued: true}, 80, []string{"Queued steer"})
	if got := len(m.itemRenderCache); got != 0 {
		t.Fatalf("expected queued user message not to be cached, got %d entries", got)
	}

	m.setCachedRenderedItem(&session.UserMessage{Content: "steer", Queued: false}, 80, []string{"steer"})
	if got := len(m.itemRenderCache); got != 1 {
		t.Fatalf("expected normal user message to be cached, got %d entries", got)
	}
}
