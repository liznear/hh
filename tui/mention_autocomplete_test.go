package tui

import (
	"os"
	"path/filepath"
	"strings"
	"testing"

	tea "charm.land/bubbletea/v2"
	"github.com/liznear/hh/agent"
	"github.com/liznear/hh/config"
)

func TestBuildMentionSuggestions_SubAgentsBeforePathsAndLimit(t *testing.T) {
	origSubAgents := listSubAgents
	origPaths := listMentionPaths
	defer func() {
		listSubAgents = origSubAgents
		listMentionPaths = origPaths
	}()

	listSubAgents = func() ([]string, error) {
		return []string{"explorer", "tester"}, nil
	}
	listMentionPaths = func(string) ([]mentionPath, error) {
		return []mentionPath{{Value: "alpha/"}, {Value: "beta.txt"}, {Value: "docs/tree.md"}, {Value: "target.txt"}, {Value: "tmp/file.txt"}, {Value: "todo.md"}}, nil
	}

	suggestions, err := buildMentionSuggestions("", ".")
	if err != nil {
		t.Fatalf("buildMentionSuggestions returned error: %v", err)
	}

	got := make([]string, 0, len(suggestions))
	for _, s := range suggestions {
		got = append(got, s.Value)
	}
	want := []string{"explorer", "tester", "alpha/", "beta.txt", "docs/tree.md"}
	if strings.Join(got, ",") != strings.Join(want, ",") {
		t.Fatalf("suggestions = %v, want %v", got, want)
	}
}

func TestBuildMentionSuggestions_IgnoredPathsComeAfterRegularPaths(t *testing.T) {
	origSubAgents := listSubAgents
	origPaths := listMentionPaths
	defer func() {
		listSubAgents = origSubAgents
		listMentionPaths = origPaths
	}()

	listSubAgents = func() ([]string, error) {
		return nil, nil
	}
	listMentionPaths = func(string) ([]mentionPath, error) {
		return []mentionPath{
			{Value: "aa-regular.txt", Ignored: false},
			{Value: "bb-ignored.txt", Ignored: true},
			{Value: "cc-regular.txt", Ignored: false},
			{Value: "dd-ignored.txt", Ignored: true},
		}, nil
	}

	suggestions, err := buildMentionSuggestions("", ".")
	if err != nil {
		t.Fatalf("buildMentionSuggestions returned error: %v", err)
	}

	got := make([]string, 0, len(suggestions))
	for _, s := range suggestions {
		got = append(got, s.Value)
	}
	want := []string{"aa-regular.txt", "cc-regular.txt", "bb-ignored.txt", "dd-ignored.txt"}
	if strings.Join(got, ",") != strings.Join(want, ",") {
		t.Fatalf("suggestions = %v, want %v", got, want)
	}
}

func TestBuildMentionSuggestions_QueryMatchesAgentAndPathBasePrefix(t *testing.T) {
	origSubAgents := listSubAgents
	origPaths := listMentionPaths
	defer func() {
		listSubAgents = origSubAgents
		listMentionPaths = origPaths
	}()

	listSubAgents = func() ([]string, error) {
		return []string{"explorer", "tester"}, nil
	}
	listMentionPaths = func(string) ([]mentionPath, error) {
		return []mentionPath{{Value: "target.txt"}, {Value: "tmp/file.txt"}, {Value: "docs/tree.md"}, {Value: "todo.md"}, {Value: "zeta.md"}}, nil
	}

	suggestions, err := buildMentionSuggestions("t", ".")
	if err != nil {
		t.Fatalf("buildMentionSuggestions returned error: %v", err)
	}

	got := make([]string, 0, len(suggestions))
	for _, s := range suggestions {
		got = append(got, s.Value)
	}
	want := []string{"tester", "target.txt", "tmp/file.txt", "docs/tree.md", "todo.md"}
	if strings.Join(got, ",") != strings.Join(want, ",") {
		t.Fatalf("suggestions = %v, want %v", got, want)
	}
}

func TestUpdate_TabAppliesMentionAutocompleteBeforeAgentSwitch(t *testing.T) {
	m := newInputTestModel()
	m.modelName = "test-model"
	m.agentName = "Build"
	m.input.SetValue("@e")

	origSubAgents := listSubAgents
	origPaths := listMentionPaths
	origList := listAvailableAgents
	origUpdate := updateRunnerForAgent
	defer func() {
		listSubAgents = origSubAgents
		listMentionPaths = origPaths
		listAvailableAgents = origList
		updateRunnerForAgent = origUpdate
	}()

	listSubAgents = func() ([]string, error) {
		return []string{"explorer"}, nil
	}
	listMentionPaths = func(string) ([]mentionPath, error) {
		return nil, nil
	}
	listAvailableAgents = func() ([]string, error) {
		return []string{"Build", "Plan"}, nil
	}
	switched := false
	updateRunnerForAgent = func(_ *agent.AgentRunner, _ string, _ config.Config, _ string) error {
		switched = true
		return nil
	}

	updated, _ := m.Update(tea.KeyPressMsg(tea.Key{Code: tea.KeyTab}))
	after := updated.(*model)

	if got := after.input.Value(); got != "@explorer" {
		t.Fatalf("input = %q, want %q", got, "@explorer")
	}
	if after.agentName != "Build" {
		t.Fatalf("agentName = %q, want %q", after.agentName, "Build")
	}
	if switched {
		t.Fatal("expected tab mention autocomplete to skip agent switch")
	}
}

func TestMentionAutocompleteHeightAndRenderRequireActiveMention(t *testing.T) {
	m := newTestModel()
	m.mentionSuggestions = []mentionSuggestion{{Value: "explorer", IsAgent: true}}
	m.input.SetValue("hello")

	if got := m.mentionAutocompleteHeight(); got != 0 {
		t.Fatalf("mentionAutocompleteHeight = %d, want 0", got)
	}
	if got := m.renderMentionAutocomplete(40); got != "" {
		t.Fatalf("renderMentionAutocomplete = %q, want empty", got)
	}
}

func TestUpdate_UpDownSelectsMentionSuggestionAndTabAppliesSelection(t *testing.T) {
	m := newInputTestModel()
	m.input.SetValue("@t")
	m.input.MoveToEnd()

	origSubAgents := listSubAgents
	origPaths := listMentionPaths
	defer func() {
		listSubAgents = origSubAgents
		listMentionPaths = origPaths
	}()

	listSubAgents = func() ([]string, error) {
		return []string{"tester"}, nil
	}
	listMentionPaths = func(string) ([]mentionPath, error) {
		return []mentionPath{{Value: "target.txt"}, {Value: "tmp/file.txt"}}, nil
	}
	m.updateMentionAutocomplete()

	updated, _ := m.Update(tea.KeyPressMsg(tea.Key{Code: tea.KeyDown}))
	afterDown := updated.(*model)
	if got := afterDown.mentionSelectionIndex; got != 1 {
		t.Fatalf("mentionSelectionIndex after down = %d, want 1", got)
	}

	updated, _ = afterDown.Update(tea.KeyPressMsg(tea.Key{Code: tea.KeyTab}))
	afterTab := updated.(*model)
	if got := afterTab.input.Value(); got != "@target.txt" {
		t.Fatalf("input after tab = %q, want %q", got, "@target.txt")
	}
}

func TestUpdate_UpDownMentionSelectionClamps(t *testing.T) {
	m := newInputTestModel()
	m.input.SetValue("@t")
	m.input.MoveToEnd()

	origSubAgents := listSubAgents
	origPaths := listMentionPaths
	defer func() {
		listSubAgents = origSubAgents
		listMentionPaths = origPaths
	}()

	listSubAgents = func() ([]string, error) {
		return []string{"tester"}, nil
	}
	listMentionPaths = func(string) ([]mentionPath, error) {
		return []mentionPath{{Value: "target.txt"}}, nil
	}
	m.updateMentionAutocomplete()

	updated, _ := m.Update(tea.KeyPressMsg(tea.Key{Code: tea.KeyDown}))
	afterDown := updated.(*model)
	if got := afterDown.mentionSelectionIndex; got != 1 {
		t.Fatalf("mentionSelectionIndex after down = %d, want 1", got)
	}

	updated, _ = afterDown.Update(tea.KeyPressMsg(tea.Key{Code: tea.KeyDown}))
	afterSecondDown := updated.(*model)
	if got := afterSecondDown.mentionSelectionIndex; got != 1 {
		t.Fatalf("mentionSelectionIndex after second down = %d, want 1", got)
	}

	updated, _ = afterSecondDown.Update(tea.KeyPressMsg(tea.Key{Code: tea.KeyUp}))
	afterUp := updated.(*model)
	if got := afterUp.mentionSelectionIndex; got != 0 {
		t.Fatalf("mentionSelectionIndex after up = %d, want 0", got)
	}

	updated, _ = afterUp.Update(tea.KeyPressMsg(tea.Key{Code: tea.KeyUp}))
	afterSecondUp := updated.(*model)
	if got := afterSecondUp.mentionSelectionIndex; got != 0 {
		t.Fatalf("mentionSelectionIndex after second up = %d, want 0", got)
	}
}

func TestCollectMentionedFileContents(t *testing.T) {
	tempDir := t.TempDir()
	filePath := filepath.Join(tempDir, "note.txt")
	if err := os.WriteFile(filePath, []byte("hello file"), 0o644); err != nil {
		t.Fatalf("write file: %v", err)
	}

	m := newTestModel()
	m.workingDir = tempDir

	origSubAgents := listSubAgents
	defer func() { listSubAgents = origSubAgents }()
	listSubAgents = func() ([]string, error) {
		return []string{"explorer"}, nil
	}

	files := m.collectMentionedFileContents("review @note.txt and @explorer")
	if len(files) != 1 {
		t.Fatalf("mentioned files len = %d, want 1", len(files))
	}
	if files[0].Path != "note.txt" {
		t.Fatalf("mentioned file path = %q, want %q", files[0].Path, "note.txt")
	}
	if files[0].Content != "hello file" {
		t.Fatalf("mentioned file content = %q, want %q", files[0].Content, "hello file")
	}
}
