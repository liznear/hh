package tui

import (
	"strings"
	"testing"
	"time"

	"charm.land/bubbles/v2/spinner"
	"charm.land/bubbles/v2/stopwatch"
	tea "charm.land/bubbletea/v2"
	"github.com/liznear/hh/tui/commands"
	"github.com/liznear/hh/tui/session"
)

func TestParseSlashInvocation(t *testing.T) {
	tests := []struct {
		name     string
		prompt   string
		wantOK   bool
		wantName string
		wantArgs []string
	}{
		{name: "plain prompt", prompt: "hello", wantOK: false},
		{name: "slash only", prompt: "/", wantOK: false},
		{name: "command only", prompt: "/new", wantOK: true, wantName: "new"},
		{name: "command with args", prompt: "/new now please", wantOK: true, wantName: "new", wantArgs: []string{"now", "please"}},
		{name: "mixed case command", prompt: " /NeW ", wantOK: true, wantName: "new"},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			got, ok := commands.ParseInvocation(tt.prompt)
			if ok != tt.wantOK {
				t.Fatalf("ok = %v, want %v", ok, tt.wantOK)
			}
			if !tt.wantOK {
				return
			}
			if got.Name != tt.wantName {
				t.Fatalf("name = %q, want %q", got.Name, tt.wantName)
			}
			if strings.Join(got.Args, " ") != strings.Join(tt.wantArgs, " ") {
				t.Fatalf("args = %v, want %v", got.Args, tt.wantArgs)
			}
		})
	}
}

func TestUpdate_NewSlashCommandStartsFreshSession(t *testing.T) {
	state := session.NewState("test-model")
	state.StartTurn().AddItem(&session.UserMessage{Content: "hello"})

	m := &model{
		modelName:       "test-model",
		theme:           DefaultTheme(),
		input:           newTextareaInput(),
		spinner:         spinner.New(spinner.WithSpinner(spinner.Dot)),
		stopwatch:       stopwatch.New(stopwatch.WithInterval(time.Second)),
		session:         state,
		toolCalls:       map[string]*session.ToolCallItem{"tool-1": {ID: "tool-1"}},
		slashCommands:   commands.BuiltIn(),
		markdownCache:   map[string]string{},
		itemRenderCache: map[uintptr]itemRenderCacheEntry{},
	}
	m.input.SetValue("/new")

	updated, _ := m.Update(tea.KeyPressMsg(tea.Key{Code: tea.KeyEnter}))
	after := updated.(*model)

	if after.session == state {
		t.Fatal("expected session pointer to be replaced")
	}
	if got := len(after.session.Turns); got != 0 {
		t.Fatalf("turn count = %d, want 0", got)
	}
	if len(after.toolCalls) != 0 {
		t.Fatalf("expected tool calls to be cleared, got %d", len(after.toolCalls))
	}
	if after.runtime.busy {
		t.Fatal("expected slash command to not start busy run")
	}
	if got := after.input.Value(); got != "" {
		t.Fatalf("input value = %q, want empty", got)
	}
}

func TestUpdate_UnknownSlashCommandShowsErrorItem(t *testing.T) {
	m := &model{
		modelName:       "test-model",
		theme:           DefaultTheme(),
		input:           newTextareaInput(),
		spinner:         spinner.New(spinner.WithSpinner(spinner.Dot)),
		stopwatch:       stopwatch.New(stopwatch.WithInterval(time.Second)),
		session:         session.NewState("test-model"),
		toolCalls:       map[string]*session.ToolCallItem{},
		slashCommands:   commands.BuiltIn(),
		markdownCache:   map[string]string{},
		itemRenderCache: map[uintptr]itemRenderCacheEntry{},
	}
	m.input.SetValue("/does-not-exist")

	updated, _ := m.Update(tea.KeyPressMsg(tea.Key{Code: tea.KeyEnter}))
	after := updated.(*model)

	last := after.session.LastItem()
	errItem, ok := last.(*session.ErrorItem)
	if !ok {
		t.Fatalf("expected last item to be error, got %T", last)
	}
	if !strings.Contains(errItem.Message, "unknown slash command") {
		t.Fatalf("unexpected error message: %q", errItem.Message)
	}
	if after.runtime.busy {
		t.Fatal("expected unknown slash command to not start busy run")
	}
}
