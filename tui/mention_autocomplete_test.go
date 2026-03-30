package tui

import (
	"os"
	"os/exec"
	"path/filepath"
	"strings"
	"testing"

	tea "charm.land/bubbletea/v2"
	"github.com/liznear/hh/agent"
	"github.com/liznear/hh/config"
)

func TestBuildMentionSuggestions_SubAgentsBeforePathsAndLimit(t *testing.T) {
	origSubAgents := listSubAgents
	origGlob := globMentionPaths
	defer func() {
		listSubAgents = origSubAgents
		globMentionPaths = origGlob
	}()

	listSubAgents = func() ([]string, error) {
		return []string{"explorer", "tester"}, nil
	}
	globMentionPaths = func(string, string) ([]mentionPath, error) {
		return []mentionPath{
			{Value: "alpha/"},
			{Value: "beta.txt"},
			{Value: "docs/tree.md"},
			{Value: "target.txt"},
			{Value: "tmp/file.txt"},
			{Value: "todo.md"},
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
	want := []string{"explorer", "tester", "alpha/", "beta.txt", "docs/tree.md"}
	if strings.Join(got, ",") != strings.Join(want, ",") {
		t.Fatalf("suggestions = %v, want %v", got, want)
	}
}

func TestBuildMentionSuggestions_IgnoredPathsComeAfterRegularPaths(t *testing.T) {
	origSubAgents := listSubAgents
	origGlob := globMentionPaths
	defer func() {
		listSubAgents = origSubAgents
		globMentionPaths = origGlob
	}()

	listSubAgents = func() ([]string, error) {
		return nil, nil
	}
	globMentionPaths = func(string, string) ([]mentionPath, error) {
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
	origGlob := globMentionPaths
	defer func() {
		listSubAgents = origSubAgents
		globMentionPaths = origGlob
	}()

	listSubAgents = func() ([]string, error) {
		return []string{"explorer", "tester"}, nil
	}
	globMentionPaths = func(string, string) ([]mentionPath, error) {
		return []mentionPath{
			{Value: "target.txt"},
			{Value: "tmp/file.txt"},
			{Value: "docs/tree.md"},
			{Value: "todo.md"},
			{Value: "zeta.md"},
		}, nil
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
	origGlob := globMentionPaths
	origList := listAvailableAgents
	origUpdate := updateRunnerForAgent
	defer func() {
		listSubAgents = origSubAgents
		globMentionPaths = origGlob
		listAvailableAgents = origList
		updateRunnerForAgent = origUpdate
	}()

	listSubAgents = func() ([]string, error) {
		return []string{"explorer"}, nil
	}
	globMentionPaths = func(string, string) ([]mentionPath, error) {
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
	origGlob := globMentionPaths
	defer func() {
		listSubAgents = origSubAgents
		globMentionPaths = origGlob
	}()

	listSubAgents = func() ([]string, error) {
		return []string{"tester"}, nil
	}
	globMentionPaths = func(string, string) ([]mentionPath, error) {
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
	origGlob := globMentionPaths
	defer func() {
		listSubAgents = origSubAgents
		globMentionPaths = origGlob
	}()

	listSubAgents = func() ([]string, error) {
		return []string{"tester"}, nil
	}
	globMentionPaths = func(string, string) ([]mentionPath, error) {
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

func TestGlobMentionPaths_UsesGlobNotWalk(t *testing.T) {
	// Create a temp directory with a known structure and verify globMentionPaths
	// returns matching entries without walking the entire tree.
	tempDir := t.TempDir()
	dirs := []string{
		"alpha",
		"docs",
	}
	for _, d := range dirs {
		if err := os.MkdirAll(filepath.Join(tempDir, d), 0o755); err != nil {
			t.Fatal(err)
		}
	}
	files := map[string]string{
		"alpha/a.txt":  "a",
		"beta.txt":     "b",
		"target.txt":   "t1",
		"tmp/file.txt": "t2",
		"docs/tree.md": "tree",
		"todo.md":      "todo",
		"zeta.md":      "z",
	}
	for name, content := range files {
		full := filepath.Join(tempDir, name)
		if err := os.MkdirAll(filepath.Dir(full), 0o755); err != nil {
			t.Fatal(err)
		}
		if err := os.WriteFile(full, []byte(content), 0o644); err != nil {
			t.Fatal(err)
		}
	}

	paths, err := globMentionPaths(tempDir, "t")
	if err != nil {
		t.Fatalf("globMentionPaths error: %v", err)
	}

	got := make([]string, 0, len(paths))
	for _, p := range paths {
		got = append(got, p.Value)
	}
	// Should find entries whose path starts with "t" or basename starts with "t"
	for _, expected := range []string{"target.txt", "tmp/file.txt", "docs/tree.md", "todo.md"} {
		found := false
		for _, g := range got {
			if g == expected {
				found = true
				break
			}
		}
		if !found {
			t.Fatalf("expected %q in results, got %v", expected, got)
		}
	}
}

func TestGlobMentionPaths_GitIgnoresExcludedFiles(t *testing.T) {
	// Create a git repo with a .gitignore so we test the git ls-files path.
	tempDir := t.TempDir()
	run := func(name string, args ...string) {
		cmd := exec.Command(name, args...)
		cmd.Dir = tempDir
		if err := cmd.Run(); err != nil {
			t.Fatalf("run %s %v: %v", name, args, err)
		}
	}
	run("git", "init")
	run("git", "config", "user.email", "test@test.com")
	run("git", "config", "user.name", "test")

	// Write .gitignore before adding files.
	if err := os.WriteFile(filepath.Join(tempDir, ".gitignore"), []byte("ignored.txt\nbuild/\n"), 0o644); err != nil {
		t.Fatal(err)
	}

	// Create files.
	for _, name := range []string{"main.go", "ignored.txt", "build/output.bin"} {
		full := filepath.Join(tempDir, name)
		if err := os.MkdirAll(filepath.Dir(full), 0o755); err != nil {
			t.Fatal(err)
		}
		if err := os.WriteFile(full, []byte("x"), 0o644); err != nil {
			t.Fatal(err)
		}
	}
	run("git", "add", ".gitignore", "main.go")
	run("git", "commit", "-m", "init")

	paths, err := globMentionPaths(tempDir, "")
	if err != nil {
		t.Fatalf("globMentionPaths error: %v", err)
	}

	got := make([]string, 0, len(paths))
	for _, p := range paths {
		got = append(got, p.Value)
	}

	// main.go and .gitignore should be present; ignored.txt and build/ should not.
	for _, unexpected := range []string{"ignored.txt", "build/output.bin", "build/"} {
		for _, g := range got {
			if g == unexpected {
				t.Fatalf("found ignored path %q in results %v", unexpected, got)
			}
		}
	}

	found := false
	for _, g := range got {
		if g == "main.go" {
			found = true
			break
		}
	}
	if !found {
		t.Fatalf("expected main.go in results, got %v", got)
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
