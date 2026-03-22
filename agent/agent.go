package agent

import (
	"context"
	"strings"
	"sync"
)

type State struct {
	SystemPrompt string
	Messages     []Message
	Tools        map[string]Tool
	titleMu      sync.Mutex
	titleReady   bool
	titlePending bool
}

type AgentRunner struct {
	model    string
	provider Provider
	state    *State
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
		Tools: a.state.Tools,
	}
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
