package tui

import (
	"strings"
	"testing"
	"time"

	tea "charm.land/bubbletea/v2"
	"github.com/liznear/hh/config"
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

	m := newTestModel()
	m.session = state
	m.toolCalls = map[string]*session.ToolCallItem{"tool-1": {ID: "tool-1"}}
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
	if after.busy {
		t.Fatal("expected slash command to not start busy run")
	}
	if got := after.input.Value(); got != "" {
		t.Fatalf("input value = %q, want empty", got)
	}
}

func TestUpdate_UnknownSlashCommandShowsErrorItem(t *testing.T) {
	m := newTestModel()
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
	if after.busy {
		t.Fatal("expected unknown slash command to not start busy run")
	}
}

func TestUpdate_ResumeSlashCommandOpensPickerSortedByCreatedAtDesc(t *testing.T) {
	tempDir := t.TempDir()
	store, err := session.NewStorage(tempDir)
	if err != nil {
		t.Fatalf("failed to create storage: %v", err)
	}

	oldState := session.NewState("test-model")
	oldState.ID = "old-session"
	oldState.SetTitle("Old session")
	oldState.CreatedAt = time.Now().Add(-2 * time.Hour)
	if err := store.SaveMeta(oldState); err != nil {
		t.Fatalf("failed to save old session meta: %v", err)
	}

	newState := session.NewState("test-model")
	newState.ID = "new-session"
	newState.SetTitle("New session")
	newState.CreatedAt = time.Now().Add(-1 * time.Hour)
	if err := store.SaveMeta(newState); err != nil {
		t.Fatalf("failed to save new session meta: %v", err)
	}

	untitledState := session.NewState("test-model")
	untitledState.ID = "untitled-session"
	untitledState.Title = "   "
	untitledState.CreatedAt = time.Now()
	if err := store.SaveMeta(untitledState); err != nil {
		t.Fatalf("failed to save untitled session meta: %v", err)
	}

	m := newTestModel()
	m.storage = store
	m.input.SetValue("/resume")

	updated, _ := m.Update(tea.KeyPressMsg(tea.Key{Code: tea.KeyEnter}))
	after := updated.(*model)

	if after.resumePicker == nil {
		t.Fatal("expected /resume to open resume picker")
	}
	if got := len(after.resumePicker.sessions); got != 3 {
		t.Fatalf("session count = %d, want 3", got)
	}
	if got := strings.TrimSpace(after.resumePicker.sessions[0].Title); got != "" {
		t.Fatalf("first title = %q, want blank (untitled)", got)
	}
	if got := after.resumePicker.sessions[1].Title; got != "New session" {
		t.Fatalf("second title = %q, want %q", got, "New session")
	}
	if got := after.resumePicker.sessions[2].Title; got != "Old session" {
		t.Fatalf("third title = %q, want %q", got, "Old session")
	}
	if after.busy {
		t.Fatal("expected /resume to not start busy run")
	}
}

func TestUpdate_ResumeSlashCommandSelectLoadsSession(t *testing.T) {
	tempDir := t.TempDir()
	store, err := session.NewStorage(tempDir)
	if err != nil {
		t.Fatalf("failed to create storage: %v", err)
	}

	selectedState := session.NewState("resumed-model")
	selectedState.ID = "selected-session"
	selectedState.SetTitle("Selected")
	selectedState.CreatedAt = time.Now()
	turn := selectedState.StartTurn()
	turn.AddItem(&session.UserMessage{Content: "resume me"})
	turn.AddItem(&session.AssistantMessage{Content: "loaded"})
	if err := store.Save(selectedState); err != nil {
		t.Fatalf("failed to save selected session: %v", err)
	}

	otherState := session.NewState("test-model")
	otherState.ID = "older-session"
	otherState.SetTitle("Older")
	otherState.CreatedAt = time.Now().Add(-1 * time.Hour)
	if err := store.SaveMeta(otherState); err != nil {
		t.Fatalf("failed to save other session meta: %v", err)
	}

	m := newTestModel()
	m.storage = store
	m.session = session.NewState("test-model")
	m.session.StartTurn().AddItem(&session.UserMessage{Content: "current"})
	m.input.SetValue("/resume")

	updated, _ := m.Update(tea.KeyPressMsg(tea.Key{Code: tea.KeyEnter}))
	afterOpen := updated.(*model)
	if afterOpen.resumePicker == nil {
		t.Fatal("expected resume picker to be open")
	}

	updated, _ = afterOpen.Update(tea.KeyPressMsg(tea.Key{Code: tea.KeyEnter}))
	afterSelect := updated.(*model)
	if afterSelect.resumePicker != nil {
		t.Fatal("expected resume picker to close after selecting session")
	}
	if afterSelect.session.ID != "selected-session" {
		t.Fatalf("session id = %q, want %q", afterSelect.session.ID, "selected-session")
	}
	if afterSelect.session.Title != "Selected" {
		t.Fatalf("session title = %q, want %q", afterSelect.session.Title, "Selected")
	}
	last := afterSelect.session.LastItem()
	assistantMsg, ok := last.(*session.AssistantMessage)
	if !ok || assistantMsg.Content != "loaded" {
		t.Fatalf("last item = %T (%v), want assistant message with loaded content", last, last)
	}
	if afterSelect.modelName != "resumed-model" {
		t.Fatalf("modelName = %q, want %q", afterSelect.modelName, "resumed-model")
	}
}

func TestUpdate_ResumeSlashCommandWithNoSessionsShowsEmptyMessage(t *testing.T) {
	tempDir := t.TempDir()
	store, err := session.NewStorage(tempDir)
	if err != nil {
		t.Fatalf("failed to create storage: %v", err)
	}

	m := newTestModel()
	m.storage = store
	m.input.SetValue("/resume")

	updated, _ := m.Update(tea.KeyPressMsg(tea.Key{Code: tea.KeyEnter}))
	after := updated.(*model)

	last := after.session.LastItem()
	msg, ok := last.(*session.AssistantMessage)
	if !ok {
		t.Fatalf("expected last item to be assistant message, got %T", last)
	}
	if msg.Content != "No sessions found for current path." {
		t.Fatalf("content = %q, want %q", msg.Content, "No sessions found for current path.")
	}
}

func TestUpdate_ModelSlashCommandOpensPickerAndSwitchesModel(t *testing.T) {
	m := newTestModel()
	m.modelName = "proxy/glm-5"
	m.session = session.NewState("proxy/glm-5")
	m.config = config.Config{
		Providers: map[string]config.ProviderConfig{
			"proxy": {
				Models: map[string]config.ModelConfig{
					"glm-5":         {},
					"gpt-5.3-codex": {},
				},
			},
		},
	}
	m.input.SetValue("/model")

	updated, _ := m.Update(tea.KeyPressMsg(tea.Key{Code: tea.KeyEnter}))
	afterOpen := updated.(*model)
	if afterOpen.modelPicker == nil {
		t.Fatal("expected /model to open model picker")
	}

	updated, _ = afterOpen.Update(tea.KeyPressMsg(tea.Key{Code: tea.KeyDown}))
	afterMove := updated.(*model)
	if afterMove.modelPicker == nil || afterMove.modelPicker.index != 1 {
		t.Fatalf("picker index = %v, want 1", afterMove.modelPicker)
	}

	updated, _ = afterMove.Update(tea.KeyPressMsg(tea.Key{Code: tea.KeyEnter}))
	afterSelect := updated.(*model)
	if afterSelect.modelPicker != nil {
		t.Fatal("expected picker to close after selecting model")
	}
	if afterSelect.modelName != "proxy/gpt-5.3-codex" {
		t.Fatalf("modelName = %q, want %q", afterSelect.modelName, "proxy/gpt-5.3-codex")
	}
	if afterSelect.session.CurrentModel != "proxy/gpt-5.3-codex" {
		t.Fatalf("session current model = %q, want %q", afterSelect.session.CurrentModel, "proxy/gpt-5.3-codex")
	}
}
