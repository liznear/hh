package tui

import (
	"context"
	"strings"
	"testing"

	tea "charm.land/bubbletea/v2"
	"github.com/liznear/hh/agent"
	"github.com/liznear/hh/config"
	"github.com/liznear/hh/tui/session"
)

type stubProvider struct{}

func (stubProvider) ChatCompletionStream(_ context.Context, _ agent.ProviderRequest, _ func(agent.ProviderStreamEvent) error) (agent.ProviderResponse, error) {
	return agent.ProviderResponse{}, nil
}

func newInputTestModel() *model {
	return newTestModel()
}

func TestUpdate_ShiftEnterInsertsNewline(t *testing.T) {
	m := newInputTestModel()
	m.input.SetValue("hello")

	updated, _ := m.Update(tea.KeyPressMsg(tea.Key{Code: tea.KeyEnter, Mod: tea.ModShift}))
	after := updated.(*model)

	if got := after.input.Value(); got != "hello\n" {
		t.Fatalf("input = %q, want %q", got, "hello\\n")
	}
}

func TestUpdate_QuestionDialogEnterWithoutRunnerShowsError(t *testing.T) {
	m := newInputTestModel()
	m.questionDialog = &questionDialogState{
		request: agent.InteractionRequest{
			InteractionID: "interaction_1",
			Kind:          agent.InteractionKindQuestion,
			Title:         "Pick one",
			Options:       []agent.InteractionOption{{ID: "option_1", Title: "A", Description: "desc"}},
		},
	}

	updated, _ := m.Update(tea.KeyPressMsg(tea.Key{Code: tea.KeyEnter}))
	after := updated.(*model)
	if after.questionDialog == nil {
		t.Fatal("expected question dialog to remain open")
	}
	if got := after.questionDialog.errorMessage; got != "runner unavailable" {
		t.Fatalf("error = %q, want %q", got, "runner unavailable")
	}
}

func TestHandleAgentEvent_MessageUserAppendsUserMessage(t *testing.T) {
	m := newInputTestModel()
	m.session.StartTurn()

	m.handleAgentEvent(agent.Event{
		Type: agent.EventTypeMessage,
		Data: agent.EventDataMessage{Message: agent.Message{Role: agent.RoleUser, Content: "from event"}},
	})

	last := m.session.LastItem()
	msg, ok := last.(*session.UserMessage)
	if !ok {
		t.Fatalf("last item type = %T, want *session.UserMessage", last)
	}
	if msg.Content != "from event" {
		t.Fatalf("user message content = %q, want %q", msg.Content, "from event")
	}
}

func TestUpdate_EnterDoesNotDirectlyAppendUserMessage(t *testing.T) {
	m := newInputTestModel()
	m.input.SetValue("hello")

	updated, _ := m.Update(tea.KeyPressMsg(tea.Key{Code: tea.KeyEnter}))
	after := updated.(*model)

	turn := after.session.CurrentTurn()
	if turn == nil {
		t.Fatal("expected turn to be started")
	}
	for _, item := range turn.Items {
		if _, ok := item.(*session.UserMessage); ok {
			t.Fatal("did not expect direct user message append on submit")
		}
	}
}

func TestHandleAgentEvent_MessageUserClearsQueuedSteering(t *testing.T) {
	m := newInputTestModel()
	m.busy = true
	m.queuedSteering = []queuedSteeringMessage{{Content: "queued one"}, {Content: "queued two"}}
	m.session.StartTurn()

	m.handleAgentEvent(agent.Event{
		Type: agent.EventTypeMessage,
		Data: agent.EventDataMessage{Message: agent.Message{Role: agent.RoleUser, Content: "applied"}},
	})

	if len(m.queuedSteering) != 0 {
		t.Fatalf("expected queued steering to be cleared, got %d", len(m.queuedSteering))
	}
}

func TestHandleAgentEvent_TurnStartClearsQueuedSteering(t *testing.T) {
	m := newInputTestModel()
	m.queuedSteering = []queuedSteeringMessage{{Content: "queued one"}}

	m.handleAgentEvent(agent.Event{Type: agent.EventTypeTurnStart})

	if len(m.queuedSteering) != 0 {
		t.Fatalf("expected queued steering to be cleared on turn start, got %d", len(m.queuedSteering))
	}
}

func TestHandleAgentEvent_TurnEndClearsQueuedSteering(t *testing.T) {
	m := newInputTestModel()
	m.queuedSteering = []queuedSteeringMessage{{Content: "queued one"}}

	m.handleAgentEvent(agent.Event{Type: agent.EventTypeTurnEnd})

	if len(m.queuedSteering) != 0 {
		t.Fatalf("expected queued steering to be cleared on turn end, got %d", len(m.queuedSteering))
	}
}

func TestUpdate_TabSwitchesAgentAndStatusReflectsSelection(t *testing.T) {
	m := newInputTestModel()
	m.modelName = "test-model"
	m.agentName = "Build"
	m.config = config.Config{}
	m.runner = agent.NewAgentRunner("test-model", stubProvider{})

	originalList := listAvailableAgents
	originalUpdate := updateRunnerForAgent
	defer func() {
		listAvailableAgents = originalList
		updateRunnerForAgent = originalUpdate
	}()

	listAvailableAgents = func() ([]string, error) {
		return []string{"Build", "Plan"}, nil
	}
	updateRunnerForAgent = func(runner *agent.AgentRunner, agentName string, cfg config.Config, workingDir string) error {
		return nil
	}

	updated, _ := m.Update(tea.KeyPressMsg(tea.Key{Code: tea.KeyTab}))
	after := updated.(*model)

	if after.agentName != "Plan" {
		t.Fatalf("agentName = %q, want %q", after.agentName, "Plan")
	}

	status := renderStatusWidget(statusWidgetModel{AgentName: after.agentName, ModelName: after.modelName}, DefaultTheme())
	if !strings.Contains(status, "Plan") {
		t.Fatalf("expected status line to include switched agent, got %q", status)
	}
}

func TestUpdate_TaskSessionViewDisablesInputEditing(t *testing.T) {
	m := newInputTestModel()
	m.input.SetValue("")
	m.taskSessionView = &taskSessionViewState{Session: session.NewState("task-session")}

	updated, _ := m.Update(tea.KeyPressMsg(tea.Key{Text: "a"}))
	after := updated.(*model)

	if got := after.input.Value(); got != "" {
		t.Fatalf("input = %q, want empty while task session view active", got)
	}
}

func TestUpdate_EscClosesTaskSessionViewBeforeBusyCancel(t *testing.T) {
	m := newInputTestModel()
	m.busy = true
	m.taskSessionView = &taskSessionViewState{Session: session.NewState("task-session")}

	updated, _ := m.Update(tea.KeyPressMsg(tea.Key{Code: tea.KeyEscape}))
	after := updated.(*model)

	if after.taskSessionView != nil {
		t.Fatal("expected task session view to close on Esc")
	}
	if after.escPending {
		t.Fatal("expected escPending to remain false when closing task session view")
	}
}

func TestUpdate_TabDoesNotSwitchWhileBusy(t *testing.T) {
	m := newInputTestModel()
	m.modelName = "test-model"
	m.agentName = "Build"
	m.config = config.Config{}
	m.runner = agent.NewAgentRunner("test-model", stubProvider{})
	m.busy = true

	originalList := listAvailableAgents
	defer func() {
		listAvailableAgents = originalList
	}()
	listAvailableAgents = func() ([]string, error) {
		return []string{"Build", "Plan"}, nil
	}

	updated, _ := m.Update(tea.KeyPressMsg(tea.Key{Code: tea.KeyTab}))
	after := updated.(*model)

	if after.agentName != "Build" {
		t.Fatalf("agentName = %q, want unchanged %q", after.agentName, "Build")
	}
}
