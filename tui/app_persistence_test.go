package tui

import (
	"os"
	"path/filepath"
	"testing"

	"github.com/liznear/hh/tui/session"
)

func TestPersistState_CreatesFilesOnlyAfterFirstUserPrompt(t *testing.T) {
	tempDir := t.TempDir()
	store, err := session.NewStorage(tempDir)
	if err != nil {
		t.Fatalf("failed to create storage: %v", err)
	}

	state := session.NewState("test-model")
	m := &model{
		session: state,
		storage: store,
	}

	metaPath := filepath.Join(tempDir, state.ID+".meta.json")
	itemsPath := filepath.Join(tempDir, state.ID+".jsonl")

	m.persistState()
	if _, err := os.Stat(metaPath); !os.IsNotExist(err) {
		t.Fatalf("expected no meta file before first user prompt, err=%v", err)
	}
	if _, err := os.Stat(itemsPath); !os.IsNotExist(err) {
		t.Fatalf("expected no items file before first user prompt, err=%v", err)
	}

	m.addItem(&session.UserMessage{Content: "hello"})

	if _, err := os.Stat(metaPath); err != nil {
		t.Fatalf("expected meta file after first user prompt, err=%v", err)
	}
	if _, err := os.Stat(itemsPath); err != nil {
		t.Fatalf("expected items file after first user prompt, err=%v", err)
	}
}
