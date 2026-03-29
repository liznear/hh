package tui

import (
	"context"
	"strings"
	"testing"

	tea "charm.land/bubbletea/v2"
	"github.com/liznear/hh/config"
)

func TestParseSubAgentInvocation(t *testing.T) {
	origResolver := resolveSubAgentCanonicalName
	defer func() { resolveSubAgentCanonicalName = origResolver }()
	resolveSubAgentCanonicalName = func(name string) (string, bool) {
		if name == "explorer" {
			return "Explorer", true
		}
		return "", false
	}

	tests := []struct {
		name       string
		prompt     string
		wantAgent  string
		wantPrompt string
		wantOK     bool
	}{
		{name: "agent only", prompt: "@explorer", wantAgent: "Explorer", wantPrompt: "", wantOK: true},
		{name: "agent with prompt", prompt: "@explorer find tests", wantAgent: "Explorer", wantPrompt: "find tests", wantOK: true},
		{name: "path mention", prompt: "@docs/plan.md", wantOK: false},
		{name: "unknown", prompt: "@unknown", wantOK: false},
		{name: "not leading mention", prompt: "hello @explorer", wantOK: false},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			agentName, taskPrompt, ok := parseSubAgentInvocation(tt.prompt)
			if ok != tt.wantOK {
				t.Fatalf("ok = %v, want %v", ok, tt.wantOK)
			}
			if agentName != tt.wantAgent {
				t.Fatalf("agentName = %q, want %q", agentName, tt.wantAgent)
			}
			if taskPrompt != tt.wantPrompt {
				t.Fatalf("taskPrompt = %q, want %q", taskPrompt, tt.wantPrompt)
			}
		})
	}
}

func TestEffectiveSubAgentPrompt_ReviewerDefaultsToUncommittedReview(t *testing.T) {
	got := effectiveSubAgentPrompt("Reviewer", "")
	if !strings.Contains(got, "Review all uncommitted changes") {
		t.Fatalf("default reviewer prompt = %q", got)
	}
}

func TestEffectiveSubAgentPrompt_NonReviewerNoDefault(t *testing.T) {
	if got := effectiveSubAgentPrompt("Explorer", ""); got != "" {
		t.Fatalf("effective prompt = %q, want empty", got)
	}
}

func TestMentionTaskLabel_ReviewerEmptyPromptUsesReviewLabel(t *testing.T) {
	if got := mentionTaskLabel("Reviewer", "", effectiveSubAgentPrompt("Reviewer", "")); got != "Review uncommitted changes" {
		t.Fatalf("task label = %q, want %q", got, "Review uncommitted changes")
	}
}

func TestHandleEnterKey_SubAgentInvocationStartsMentionRun(t *testing.T) {
	m := newInputTestModel()
	m.config = config.Config{}
	m.input.SetValue("@explorer inspect architecture")

	origResolver := resolveSubAgentCanonicalName
	origStarter := startMentionSubAgentStreamCmdWithContext
	defer func() {
		resolveSubAgentCanonicalName = origResolver
		startMentionSubAgentStreamCmdWithContext = origStarter
	}()

	resolveSubAgentCanonicalName = func(name string) (string, bool) {
		if name == "explorer" {
			return "Explorer", true
		}
		return "", false
	}

	called := false
	startMentionSubAgentStreamCmdWithContext = func(ctx context.Context, cfg config.Config, modelName, workingDir, subAgentName, taskPrompt, internalState, toolCallID string) tea.Cmd {
		called = true
		if subAgentName != "Explorer" {
			t.Fatalf("subAgentName = %q, want %q", subAgentName, "Explorer")
		}
		if taskPrompt != "inspect architecture" {
			t.Fatalf("taskPrompt = %q, want %q", taskPrompt, "inspect architecture")
		}
		if internalState == "" {
			t.Fatal("expected non-empty internalState")
		}
		if toolCallID == "" {
			t.Fatal("expected non-empty toolCallID")
		}
		return nil
	}

	updated, _ := m.Update(tea.KeyPressMsg(tea.Key{Code: tea.KeyEnter}))
	after := updated.(*model)

	if !called {
		t.Fatal("expected sub-agent mention starter to be called")
	}
	if !after.busy {
		t.Fatal("expected model to become busy")
	}
	if got := after.input.Value(); got != "" {
		t.Fatalf("input = %q, want empty", got)
	}
}

func TestHandleEnterKey_ReviewerWithoutPromptUsesDefaultReviewPrompt(t *testing.T) {
	m := newInputTestModel()
	m.config = config.Config{}
	m.input.SetValue("@reviewer")

	origResolver := resolveSubAgentCanonicalName
	origStarter := startMentionSubAgentStreamCmdWithContext
	defer func() {
		resolveSubAgentCanonicalName = origResolver
		startMentionSubAgentStreamCmdWithContext = origStarter
	}()

	resolveSubAgentCanonicalName = func(name string) (string, bool) {
		if name == "reviewer" {
			return "Reviewer", true
		}
		return "", false
	}

	capturedPrompt := ""
	startMentionSubAgentStreamCmdWithContext = func(_ context.Context, _ config.Config, _ string, _ string, subAgentName, taskPrompt, _ string, _ string) tea.Cmd {
		if subAgentName != "Reviewer" {
			t.Fatalf("subAgentName = %q, want %q", subAgentName, "Reviewer")
		}
		capturedPrompt = taskPrompt
		return nil
	}

	updated, _ := m.Update(tea.KeyPressMsg(tea.Key{Code: tea.KeyEnter}))
	after := updated.(*model)
	if !after.busy {
		t.Fatal("expected model to become busy")
	}
	if strings.TrimSpace(capturedPrompt) != "" {
		t.Fatalf("taskPrompt argument = %q, want empty raw prompt for reviewer default path", capturedPrompt)
	}
	if got := effectiveSubAgentPrompt("Reviewer", capturedPrompt); !strings.Contains(got, "Review all uncommitted changes") {
		t.Fatalf("effective reviewer prompt = %q", got)
	}
}

func TestHandleEnterKey_PathMentionDoesNotStartSubAgentRun(t *testing.T) {
	m := newInputTestModel()
	m.config = config.Config{}
	m.input.SetValue("@docs/plan.md")

	origResolver := resolveSubAgentCanonicalName
	origStarter := startMentionSubAgentStreamCmdWithContext
	defer func() {
		resolveSubAgentCanonicalName = origResolver
		startMentionSubAgentStreamCmdWithContext = origStarter
	}()

	resolveSubAgentCanonicalName = func(name string) (string, bool) { return "", false }

	mentionCalled := false
	startMentionSubAgentStreamCmdWithContext = func(context.Context, config.Config, string, string, string, string, string, string) tea.Cmd {
		mentionCalled = true
		return nil
	}

	updated, _ := m.Update(tea.KeyPressMsg(tea.Key{Code: tea.KeyEnter}))
	after := updated.(*model)

	if mentionCalled {
		t.Fatal("expected path mention not to trigger sub-agent run")
	}
	if !after.busy {
		t.Fatal("expected normal agent run path to be used")
	}
}
