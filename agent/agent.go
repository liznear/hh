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

// Event mapping contract (ProviderResponse -> Event):
//
//  1. ProviderResponse.Error
//     -> EventTypeError with EventDataError
//
//  2. ProviderResponse.ThinkingDelta
//     -> EventTypeThinkingDelta with EventDataThinkingDelta
//
//  3. ProviderResponse.MessageDelta
//     -> EventTypeMessageDelta with EventDataMessageDelta
//
//  4. ProviderResponse.ToolCallDelta
//     -> EventTypeToolCallDelta with EventDataToolCallDelta
//
//  5. ProviderResponse.Message
//     -> EventTypeMessage with EventDataMessage
//
//  6. ProviderResponse.ToolCalls
//     -> EventTypeToolCalls with EventDataToolCalls
//
//  7. ProviderResponse.FinishReason (!= FinishReasonUnknown)
//     -> EventTypeDone with EventDataDone
//
// Ordering notes:
// - A single ProviderResponse may emit multiple Events (for example, delta + done).
// - Error is emitted first for that response and remaining mappings are skipped.
// - The EventStream closes when the provider response channel is exhausted.

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
			// Provider-level error maps directly to an error event and short-circuits
			// further mapping for this response item.
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
