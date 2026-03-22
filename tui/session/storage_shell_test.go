package session

import "testing"

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
