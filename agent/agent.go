package agent

import "context"

type Agent struct {
	provider Provider
}

type Context struct {
	SystemPrompt string
	History      []Message
	Prompts      []Message
	Tools        []Tool
}

func (a *Agent) Run(ctx context.Context, aCtx Context) EventStream {
	req := ProviderRequest{
		Messages: []Message{{RoleSystem, aCtx.SystemPrompt, ""}},
		Tools:    aCtx.Tools,
	}
	req.Messages = append(req.Messages, aCtx.History...)
	req.Messages = append(req.Messages, aCtx.Prompts...)

	ch := make(chan Event, 1)
	ret := EventStream{
		ch: ch,
	}

	resCh, err := a.provider.ChatCompletionStream(ctx, req)
	go func() {
		defer close(resCh)
		defer close(ch)
		if err != nil {
			ch <- Event{EventTypeError, err}
			return
		}
		for res := range resCh {
			ch <- Event{EventTypeThinkingDelta, res}
		}
	}()
	return ret
}
