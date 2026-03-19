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
		defer close(ch)
		if err != nil {
			ch <- Event{Type: EventTypeError, Data: EventDataError{Err: err}}
			return
		}
		for res := range resCh {
			if res.Error != nil {
				ch <- Event{Type: EventTypeError, Data: EventDataError{Err: res.Error}}
				continue
			}
			if res.ThinkingDelta != "" {
				ch <- Event{Type: EventTypeThinkingDelta, Data: EventDataThinkingDelta{Delta: res.ThinkingDelta}}
			}
			if res.MessageDelta != "" {
				ch <- Event{Type: EventTypeMessageDelta, Data: EventDataMessageDelta{Delta: res.MessageDelta}}
			}
			if res.ToolCallDelta != nil {
				ch <- Event{Type: EventTypeToolCallDelta, Data: EventDataToolCallDelta{Delta: *res.ToolCallDelta}}
			}
			if res.Message != nil {
				ch <- Event{Type: EventTypeMessage, Data: EventDataMessage{Message: *res.Message}}
			}
			if len(res.ToolCalls) > 0 {
				ch <- Event{Type: EventTypeToolCalls, Data: EventDataToolCalls{ToolCalls: res.ToolCalls}}
			}
			if res.FinishReason != FinishReasonUnknown {
				ch <- Event{Type: EventTypeDone, Data: EventDataDone{Reason: res.FinishReason}}
			}
		}
	}()
	return ret
}
