package agent

import (
	"context"
	"fmt"

	"github.com/liznear/hh/common"
)

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
		common.BridgeChannel(ctx, resCh, ch, toEvent)
	}()
	return ret
}

func toEvent(res ProviderResponse) Event {
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
	// - A single ProviderResponse only emits one Events.
	//
	// Provider-level error maps directly to an error event and short-circuits
	// further mapping for this response item.
	if res.Error != nil {
		return Event{Type: EventTypeError, Data: EventDataError{Err: res.Error}}
	}
	if res.ThinkingDelta != "" {
		return Event{Type: EventTypeThinkingDelta, Data: EventDataThinkingDelta{Delta: res.ThinkingDelta}}
	}
	if res.MessageDelta != "" {
		return Event{Type: EventTypeMessageDelta, Data: EventDataMessageDelta{Delta: res.MessageDelta}}
	}
	if res.ToolCallDelta != nil {
		return Event{Type: EventTypeToolCallDelta, Data: EventDataToolCallDelta{Delta: *res.ToolCallDelta}}
	}
	if res.Message != nil {
		return Event{Type: EventTypeMessage, Data: EventDataMessage{Message: *res.Message}}
	}
	if len(res.ToolCalls) > 0 {
		return Event{Type: EventTypeToolCalls, Data: EventDataToolCalls{ToolCalls: res.ToolCalls}}
	}
	if res.FinishReason != FinishReasonUnknown {
		return Event{Type: EventTypeDone, Data: EventDataDone{Reason: res.FinishReason}}
	}
	return Event{Type: EventTypeError, Data: EventDataError{Err: fmt.Errorf("invalid response: %v", res)}}
}
