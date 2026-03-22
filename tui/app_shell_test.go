package tui

import (
	"context"
	"strings"
	"testing"
	"time"

	"charm.land/bubbles/v2/spinner"
	"charm.land/bubbles/v2/stopwatch"
	tea "charm.land/bubbletea/v2"
	"github.com/charmbracelet/x/ansi"
	"github.com/liznear/hh/tui/session"
)

func TestIsShellModeInput(t *testing.T) {
	if !isShellModeInput("!ls -al") {
		t.Fatal("expected ! prefix to enable shell mode")
	}
	if !isShellModeInput("  !pwd") {
		t.Fatal("expected leading whitespace before ! to still enable shell mode")
	}
	if isShellModeInput("hello") {
		t.Fatal("expected non-! input to not be shell mode")
	}
}

func TestParseShellCommand(t *testing.T) {
	got := parseShellCommand("  !echo hello  ")
	if got != "echo hello" {
		t.Fatalf("command = %q, want %q", got, "echo hello")
	}
	if got := parseShellCommand("hello"); got != "" {
		t.Fatalf("expected empty command for non-shell input, got %q", got)
	}
}

func TestRunShellCommandCmdWithContext(t *testing.T) {
	msg := runShellCommandCmdWithContext(context.Background(), "printf 'hello'")()
	done, ok := msg.(shellCommandDoneMsg)
	if !ok {
		t.Fatalf("unexpected msg type: %T", msg)
	}
	if done.command != "printf 'hello'" {
		t.Fatalf("command = %q", done.command)
	}
	if done.output != "hello" {
		t.Fatalf("output = %q, want %q", done.output, "hello")
	}
	if done.err != nil {
		t.Fatalf("unexpected err: %v", done.err)
	}
}

func TestUpdate_EnterInShellModeRunsCommandAndAddsShellMessage(t *testing.T) {
	m := &model{
		theme:           DefaultTheme(),
		input:           newTextareaInput(),
		spinner:         spinner.New(spinner.WithSpinner(spinner.Dot)),
		stopwatch:       stopwatch.New(stopwatch.WithInterval(time.Second)),
		session:         session.NewState("test-model"),
		toolCalls:       map[string]*session.ToolCallItem{},
		markdownCache:   map[string]string{},
		itemRenderCache: map[uintptr]itemRenderCacheEntry{},
	}
	m.input.SetValue("!printf hello")

	updated, cmd := m.Update(tea.KeyPressMsg(tea.Key{Code: tea.KeyEnter}))
	after := updated.(*model)
	if !after.runtime.busy {
		t.Fatal("expected busy run after shell submit")
	}
	if cmd == nil {
		t.Fatal("expected command to be returned")
	}

	doneMsg := shellCommandDoneMsg{command: "printf hello", output: "hello"}
	updated, _ = after.Update(doneMsg)
	afterDone := updated.(*model)
	if afterDone.runtime.busy {
		t.Fatal("expected busy to be false after shell completion")
	}

	items := afterDone.session.CurrentTurnItems()
	if len(items) < 2 {
		t.Fatalf("expected shell turn items, got %d", len(items))
	}
	var shellItem *session.ShellMessage
	for _, item := range items {
		if sm, ok := item.(*session.ShellMessage); ok {
			shellItem = sm
			break
		}
	}
	if shellItem == nil {
		t.Fatalf("expected shell message item in turn, got %#v", items)
	}
	if shellItem.Command != "printf hello" {
		t.Fatalf("command = %q", shellItem.Command)
	}
	if shellItem.Output != "hello" {
		t.Fatalf("output = %q", shellItem.Output)
	}
}

func TestUpdate_BangEntersShellModeWithoutInputBang(t *testing.T) {
	m := &model{
		theme:           DefaultTheme(),
		input:           newTextareaInput(),
		spinner:         spinner.New(spinner.WithSpinner(spinner.Dot)),
		stopwatch:       stopwatch.New(stopwatch.WithInterval(time.Second)),
		session:         session.NewState("test-model"),
		toolCalls:       map[string]*session.ToolCallItem{},
		markdownCache:   map[string]string{},
		itemRenderCache: map[uintptr]itemRenderCacheEntry{},
	}

	updated, _ := m.Update(tea.KeyPressMsg(tea.Key{Code: '!', Text: "!"}))
	after := updated.(*model)
	if !after.runtime.shellMode {
		t.Fatal("expected shell mode after typing !")
	}
	if after.input.Value() != "" {
		t.Fatalf("expected ! to be hidden from input, got %q", after.input.Value())
	}
}

func TestUpdate_BackspaceLeavesShellModeWhenInputEmpty(t *testing.T) {
	m := &model{
		theme:           DefaultTheme(),
		input:           newTextareaInput(),
		spinner:         spinner.New(spinner.WithSpinner(spinner.Dot)),
		stopwatch:       stopwatch.New(stopwatch.WithInterval(time.Second)),
		session:         session.NewState("test-model"),
		toolCalls:       map[string]*session.ToolCallItem{},
		markdownCache:   map[string]string{},
		itemRenderCache: map[uintptr]itemRenderCacheEntry{},
	}
	m.setShellMode(true)

	updated, _ := m.Update(tea.KeyPressMsg(tea.Key{Code: tea.KeyBackspace}))
	after := updated.(*model)
	if after.runtime.shellMode {
		t.Fatal("expected backspace on empty shell input to leave shell mode")
	}
}

func TestUpdate_EnterInExplicitShellModeExitsShellMode(t *testing.T) {
	m := &model{
		theme:           DefaultTheme(),
		input:           newTextareaInput(),
		spinner:         spinner.New(spinner.WithSpinner(spinner.Dot)),
		stopwatch:       stopwatch.New(stopwatch.WithInterval(time.Second)),
		session:         session.NewState("test-model"),
		toolCalls:       map[string]*session.ToolCallItem{},
		markdownCache:   map[string]string{},
		itemRenderCache: map[uintptr]itemRenderCacheEntry{},
	}
	m.setShellMode(true)
	m.input.SetValue("printf hello")

	updated, _ := m.Update(tea.KeyPressMsg(tea.Key{Code: tea.KeyEnter}))
	after := updated.(*model)
	if after.runtime.shellMode {
		t.Fatal("expected shell mode to exit after submitting a shell command")
	}
}

func TestRenderShellMessageWidget(t *testing.T) {
	m := &model{theme: DefaultTheme()}
	lines := m.renderShellMessageWidget(&session.ShellMessage{Command: "ls -al", Output: "line1\nline2"}, 40)
	if len(lines) == 0 || !strings.HasPrefix(ansi.Strip(lines[0]), "  ") {
		t.Fatalf("expected shell box to include extra left margin, got %q", ansi.Strip(strings.Join(lines, "\n")))
	}
	joined := ansi.Strip(strings.Join(lines, "\n"))
	if !strings.Contains(joined, "$ ls -al") {
		t.Fatalf("expected command in shell widget: %q", joined)
	}
	if !strings.Contains(joined, "line1") || !strings.Contains(joined, "line2") {
		t.Fatalf("expected output in shell widget: %q", joined)
	}
}
