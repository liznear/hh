package agent

import (
	"context"
	"fmt"
	"strings"
	"sync"
	"sync/atomic"
	"time"
)

type State struct {
	SystemPrompt string
	Messages     []Message
	Tools        map[string]Tool
	titleMu      sync.Mutex
	titleReady   bool
	titlePending bool
	runMu        sync.Mutex
	activeRun    *activeRun
}

type activeRun struct {
	runID        string
	interactions *InteractionManager
	steering     *SteeringQueue
}

type AgentRunner struct {
	model    string
	provider Provider
	state    *State
}

func (a *AgentRunner) SetModel(model string) {
	if a == nil {
		return
	}
	a.model = strings.TrimSpace(model)
}

func NewAgentRunner(model string, provider Provider, opts ...Opt) *AgentRunner {
	state := &State{
		Messages: nil,
	}
	for _, opt := range opts {
		opt(state)
	}
	return &AgentRunner{
		model:    model,
		provider: provider,
		state:    state,
	}
}

type Input struct {
	Content string
	Type    string
}

func (a *AgentRunner) Run(ctx context.Context, input Input, onEvent func(Event)) error {
	runID := newRunID()
	interactions := NewInteractionManager()
	steering := NewSteeringQueue()
	if err := a.setActiveRun(runID, interactions, steering); err != nil {
		return err
	}
	defer a.clearActiveRun(runID)

	titleStarted := make(chan struct{})
	titleDone := a.maybeGenerateSessionTitleAsync(ctx, input, onEvent, titleStarted)
	if titleDone != nil {
		<-titleStarted
	}

	aCtx := Context{
		Model:        a.model,
		Provider:     a.provider,
		SystemPrompt: a.state.SystemPrompt,
		History:      a.state.Messages,
		Prompts: []Message{
			{Role: RoleUser, Content: input.Content},
		},
		Tools:        a.state.Tools,
		RunID:        runID,
		Interactions: interactions,
		Steering:     steering,
	}
	onEvent(Event{
		Type:      EventTypeMessage,
		Data:      EventDataMessage{Message: Message{Role: RoleUser, Content: input.Content}},
		RunID:     runID,
		TurnID:    1,
		Timestamp: time.Now().UTC(),
	})
	RunAgentLoop(ctx, aCtx, func(event Event) {
		onEvent(event)
		switch event.Type {
		case EventTypeAgentEnd:
			a.state.Messages = event.Data.(EventDataAgentEnd).Messages
		}
	})
	if titleDone != nil {
		<-titleDone
	}
	return nil
}

func (a *AgentRunner) SubmitInteractionResponse(resp InteractionResponse) error {
	if a == nil || a.state == nil {
		return ErrNoActiveRun
	}
	a.state.runMu.Lock()
	active := a.state.activeRun
	a.state.runMu.Unlock()
	if active == nil || active.interactions == nil {
		return ErrNoActiveRun
	}
	if resp.RunID != "" && resp.RunID != active.runID {
		return ErrNoActiveRun
	}
	if resp.RunID == "" {
		resp.RunID = active.runID
	}
	return active.interactions.Submit(resp)
}

func (a *AgentRunner) DismissInteraction(interactionID, runID string) error {
	if a == nil || a.state == nil {
		return ErrNoActiveRun
	}
	a.state.runMu.Lock()
	active := a.state.activeRun
	a.state.runMu.Unlock()
	if active == nil || active.interactions == nil {
		return ErrNoActiveRun
	}
	if runID != "" && runID != active.runID {
		return ErrNoActiveRun
	}
	return active.interactions.Dismiss(interactionID)
}

func (a *AgentRunner) SubmitSteeringMessage(content, runID string) error {
	if a == nil || a.state == nil {
		return ErrNoActiveRun
	}
	a.state.runMu.Lock()
	active := a.state.activeRun
	a.state.runMu.Unlock()
	if active == nil || active.steering == nil {
		return ErrNoActiveRun
	}
	if runID != "" && runID != active.runID {
		return ErrNoActiveRun
	}
	_, err := active.steering.Enqueue(content)
	return err
}

func (a *AgentRunner) maybeGenerateSessionTitleAsync(ctx context.Context, input Input, onEvent func(Event), started chan<- struct{}) <-chan struct{} {
	if a == nil || a.state == nil || a.provider == nil {
		return nil
	}
	if !a.tryStartTitleGeneration() {
		return nil
	}

	prompt := strings.TrimSpace(input.Content)
	if prompt == "" {
		a.finishTitleGeneration(false)
		return nil
	}

	done := make(chan struct{})
	go func() {
		defer close(done)
		if started != nil {
			close(started)
		}
		title, ok := a.generateSessionTitle(ctx, prompt)
		a.finishTitleGeneration(ok)
		if !ok {
			return
		}
		onEvent(Event{Type: EventTypeSessionTitle, Data: EventDataSessionTitle{Title: title}})
	}()
	return done
}

func (a *AgentRunner) tryStartTitleGeneration() bool {
	a.state.titleMu.Lock()
	defer a.state.titleMu.Unlock()
	if a.state.titleReady || a.state.titlePending {
		return false
	}
	a.state.titlePending = true
	return true
}

func (a *AgentRunner) finishTitleGeneration(success bool) {
	a.state.titleMu.Lock()
	defer a.state.titleMu.Unlock()
	a.state.titlePending = false
	if success {
		a.state.titleReady = true
	}
}

func (a *AgentRunner) generateSessionTitle(ctx context.Context, firstPrompt string) (string, bool) {
	req := ProviderRequest{
		Model: a.model,
		Messages: []Message{
			{
				Role:    RoleSystem,
				Content: "Generate a concise session title from the user's first prompt. Return only the title, plain text, with no quotes, no punctuation suffix, and no extra explanation. Keep it within 6 words.",
			},
			{
				Role:    RoleUser,
				Content: firstPrompt,
			},
		},
	}

	res, err := a.provider.ChatCompletionStream(ctx, req, func(ProviderStreamEvent) error {
		return nil
	})
	if err != nil {
		return "", false
	}
	title := normalizeSessionTitle(res.Message.Content)
	if title == "" {
		return "", false
	}
	return title, true
}

func normalizeSessionTitle(raw string) string {
	raw = strings.TrimSpace(raw)
	if raw == "" {
		return ""
	}
	line := strings.Split(raw, "\n")[0]
	line = strings.TrimSpace(line)
	line = strings.Trim(line, `"'`)
	line = strings.TrimSpace(line)
	if line == "" {
		return ""
	}
	runes := []rune(line)
	if len(runes) > 80 {
		line = string(runes[:80])
	}
	return strings.TrimSpace(line)
}

var runIDCounter atomic.Uint64

func newRunID() string {
	id := runIDCounter.Add(1)
	return fmt.Sprintf("run_%d", id)
}

func (a *AgentRunner) setActiveRun(runID string, interactions *InteractionManager, steering *SteeringQueue) error {
	if a == nil || a.state == nil {
		return ErrNoActiveRun
	}
	a.state.runMu.Lock()
	defer a.state.runMu.Unlock()
	if a.state.activeRun != nil {
		return fmt.Errorf("run %s is already active", a.state.activeRun.runID)
	}
	a.state.activeRun = &activeRun{runID: runID, interactions: interactions, steering: steering}
	return nil
}

func (a *AgentRunner) clearActiveRun(runID string) {
	if a == nil || a.state == nil {
		return
	}
	a.state.runMu.Lock()
	defer a.state.runMu.Unlock()
	if a.state.activeRun == nil {
		return
	}
	if runID != "" && a.state.activeRun.runID != runID {
		return
	}
	a.state.activeRun = nil
}

type Opt func(*State)

func WithSystemPrompt(prompt string) Opt {
	return func(s *State) {
		s.SystemPrompt = prompt
	}
}

func WithTools(tools map[string]Tool) Opt {
	return func(s *State) {
		s.Tools = tools
	}
}
