package session

import "testing"

func TestStorage_SaveAndLoadMetaWithTodoItems(t *testing.T) {
	store, err := NewStorage(t.TempDir())
	if err != nil {
		t.Fatalf("failed to create storage: %v", err)
	}

	state := NewState("test-model")
	state.SetTitle("Investigate memory leak")
	state.SetTodoItems([]TodoItem{
		{Content: "Implement todo_write", Status: TodoStatusWIP},
		{Content: "Add tests", Status: TodoStatusPending},
	})

	if err := store.SaveMeta(state); err != nil {
		t.Fatalf("failed to save meta: %v", err)
	}

	meta, err := store.LoadMeta(state.ID)
	if err != nil {
		t.Fatalf("failed to load meta: %v", err)
	}

	if len(meta.TodoItems) != 2 {
		t.Fatalf("todo item count = %d, want 2", len(meta.TodoItems))
	}
	if meta.Title != "Investigate memory leak" {
		t.Fatalf("meta title = %q, want %q", meta.Title, "Investigate memory leak")
	}
	if meta.TodoItems[0].Status != TodoStatusWIP {
		t.Fatalf("first todo status = %q, want %q", meta.TodoItems[0].Status, TodoStatusWIP)
	}
}

func TestNewState_DefaultTitle(t *testing.T) {
	state := NewState("test-model")
	if state.Title != "Untitled Session" {
		t.Fatalf("default title = %q, want %q", state.Title, "Untitled Session")
	}
}
