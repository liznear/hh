package agent

import (
	"context"
	"fmt"

	"github.com/samber/lo"
	"golang.org/x/exp/maps"
)

type Config struct {
	provider Provider
}

type Context struct {
	Model        string
	SystemPrompt string
	History      []Message
	Prompts      []Message
	Tools        map[string]Tool
}

func RunAgentLoop(ctx context.Context, conf Config, aCtx Context, onEvent func(Event)) {
	req := ProviderRequest{
		Model:    aCtx.Model,
		Messages: []Message{{Role: RoleSystem, Content: aCtx.SystemPrompt}},
		Tools:    maps.Values(aCtx.Tools),
	}
	req.Messages = append(req.Messages, aCtx.History...)
	req.Messages = append(req.Messages, aCtx.Prompts...)

	shouldContinue := true

	onEvent(Event{EventTypeAgentStart, EventDataAgentStart{}})
	for shouldContinue {
		shouldContinue = false

		select {
		case <-ctx.Done():
			if err := ctx.Err(); err != nil {
				onEvent(Event{Type: EventTypeError, Data: err})
			}
		default:
		}

		onEvent(Event{EventTypeTurnStart, EventDataTurnStart{}})
		res, err := conf.provider.ChatCompletionStream(ctx, req, func(se ProviderStreamEvent) error {
			onEvent(toEvent(se))
			return nil
		})
		if err != nil {
			onEvent(Event{Type: EventTypeError, Data: err})
			onEvent(Event{EventTypeTurnEnd, EventDataTurnEnd{}})
		}

		if len(res.ToolCalls) > 0 {
			toolResults := executeTools(ctx, aCtx, res.ToolCalls, onEvent)
			req.Messages = append(req.Messages, lo.Map(toolResults, func(r ToolResult, idx int) Message {
				return Message{Role: RoleTool, Content: r.Data, CallID: res.ToolCalls[idx].ID}
			})...)
			shouldContinue = true
		}
		onEvent(Event{EventTypeTurnEnd, EventDataTurnEnd{}})
	}
	onEvent(Event{EventTypeAgentEnd, EventDataAgentEnd{req.Messages[1:]}})
}

func executeTools(ctx context.Context, aContext Context, toolCalls []ToolCall, onEvent func(Event)) []ToolResult {
	ret := make([]ToolResult, 0, len(toolCalls))
	for _, toolCall := range toolCalls {
		onEvent(Event{EventTypeToolCallStart, EventDataToolCallStart{toolCall}})
		toolName := toolCall.Name
		var result ToolResult
		if tool, ok := aContext.Tools[toolName]; !ok {
			result = ToolResult{IsErr: true, Data: "Not found"}
		} else {
			result = tool.Handler(ctx, toolCall.Arguments)
		}
		onEvent(Event{EventTypeToolCallEnd, EventDataToolCallEnd{toolCall, result}})
		ret = append(ret, result)
	}
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
