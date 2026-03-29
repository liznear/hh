package session

import (
	"testing"

	"github.com/liznear/hh/agent"
	"github.com/liznear/hh/tools"
)

func TestStorage_SaveAndLoadShellMessage(t *testing.T) {
	store, err := NewStorage(t.TempDir())
	if err != nil {
		t.Fatalf("failed to create storage: %v", err)
	}

	state := NewState("test-model")
	turn := state.StartTurn()
	turn.AddItem(&UserMessage{Content: "hello"})
	turn.AddItem(&ShellMessage{Command: "pwd", Output: "/tmp"})
	turn.End()

	if err := store.Save(state); err != nil {
		t.Fatalf("save failed: %v", err)
	}

	loaded, err := store.Load(state.ID)
	if err != nil {
		t.Fatalf("load failed: %v", err)
	}

	items := loaded.AllItems()
	found := false
	for _, item := range items {
		sm, ok := item.(*ShellMessage)
		if !ok {
			continue
		}
		found = true
		if sm.Command != "pwd" {
			t.Fatalf("command = %q, want %q", sm.Command, "pwd")
		}
		if sm.Output != "/tmp" {
			t.Fatalf("output = %q, want %q", sm.Output, "/tmp")
		}
	}

	if !found {
		t.Fatal("expected loaded state to include shell message")
	}
}

func TestStorage_SaveAndLoadTaskToolResultMessages(t *testing.T) {
	store, err := NewStorage(t.TempDir())
	if err != nil {
		t.Fatalf("failed to create storage: %v", err)
	}

	state := NewState("test-model")
	turn := state.StartTurn()
	turn.AddItem(&ToolCallItem{
		Name:   "task",
		Status: ToolCallStatusSuccess,
		Result: &ToolCallResult{Result: tools.TaskResult{Tasks: []tools.TaskTaskResult{{
			SubAgentName: "Explorer",
			Task:         "Inspect architecture",
			Status:       tools.TaskTaskStatusSuccess,
			Messages: []agent.Message{
				{Role: agent.RoleUser, Content: "Inspect architecture"},
				{Role: agent.RoleAssistant, Content: "Found structure"},
			},
		}}}},
	})
	turn.End()

	if err := store.Save(state); err != nil {
		t.Fatalf("save failed: %v", err)
	}

	loaded, err := store.Load(state.ID)
	if err != nil {
		t.Fatalf("load failed: %v", err)
	}

	var loadedTask *ToolCallItem
	for _, item := range loaded.AllItems() {
		if tc, ok := item.(*ToolCallItem); ok && tc.Name == "task" {
			loadedTask = tc
			break
		}
	}
	if loadedTask == nil {
		t.Fatal("expected loaded state to include task tool call")
	}

	result, ok := loadedTask.Result.Result.(tools.TaskResult)
	if !ok {
		t.Fatalf("task result type = %T, want tools.TaskResult", loadedTask.Result.Result)
	}
	if len(result.Tasks) != 1 || len(result.Tasks[0].Messages) != 2 {
		t.Fatalf("unexpected loaded task messages: %+v", result.Tasks)
	}
}
