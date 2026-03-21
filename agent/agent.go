package agent

import (
	"context"
)

type State struct {
	SystemPrompt string
	Messages     []Message
	Tools        map[string]Tool
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
	RunAgentLoop(ctx, aCtx, onEvent)
	return nil
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
