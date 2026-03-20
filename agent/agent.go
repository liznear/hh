package agent

import (
	"context"
	"fmt"
)

type Config struct {
	provider Provider
}

type Context struct {
	SystemPrompt string
	History      []Message
	Prompts      []Message
	Tools        []Tool
}

func RunAgentLoop(ctx context.Context, conf Config, aCtx Context) EventStream {
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

	go func() {
		defer close(ch)

		res, err := conf.provider.ChatCompletionStream(ctx, req, func(se ProviderStreamEvent) error {
			select {
			case ch <- toEvent(se):
				return nil
			case <-ctx.Done():
				return ctx.Err()
			}
		})

		if err != nil {
			ch <- Event{Type: EventTypeError, Data: EventDataError{Err: err}}
			return
		}

		ch <- Event{Type: EventTypeMessage, Data: EventDataMessage{Message: res.Message}}
		if len(res.ToolCalls) > 0 {
			ch <- Event{Type: EventTypeToolCalls, Data: EventDataToolCalls{ToolCalls: res.ToolCalls}}
		}
		if res.FinishReason != FinishReasonUnknown {
			ch <- Event{Type: EventTypeDone, Data: EventDataDone{Reason: res.FinishReason}}
		}
	}()

	return ret
}

func toEvent(se ProviderStreamEvent) Event {
	if se.ThinkingDelta != "" {
		return Event{Type: EventTypeThinkingDelta, Data: EventDataThinkingDelta{Delta: se.ThinkingDelta}}
	}
	if se.MessageDelta != "" {
		return Event{Type: EventTypeMessageDelta, Data: EventDataMessageDelta{Delta: se.MessageDelta}}
	}
	return Event{Type: EventTypeError, Data: EventDataError{Err: fmt.Errorf("invalid stream event: %v", se)}}
}
