package agent

import (
	"context"
	"encoding/json"
	"fmt"
	"time"

	"github.com/samber/lo"
	"golang.org/x/exp/maps"
)

func RunAgentLoop(ctx context.Context, aCtx Context, onEvent func(Event)) {
	emit := newEventEmitter(aCtx.RunID, onEvent)
	req := ProviderRequest{
		Model:    aCtx.Model,
		Messages: []Message{{Role: RoleSystem, Content: aCtx.SystemPrompt}},
		Tools:    maps.Values(aCtx.Tools),
	}
	req.Messages = append(req.Messages, aCtx.History...)
	req.Messages = append(req.Messages, aCtx.Prompts...)

	shouldContinue := true
	turnID := 0

	emit(Event{Type: EventTypeAgentStart, Data: EventDataAgentStart{}})
AgentLoop:
	for shouldContinue {
		shouldContinue = false

		select {
		case <-ctx.Done():
			if err := ctx.Err(); err != nil {
				emit(Event{Type: EventTypeError, Data: err})
			}
			break AgentLoop
		default:
		}

		turnID++
		emit(Event{Type: EventTypeTurnStart, Data: EventDataTurnStart{}, TurnID: turnID})
		res, err := aCtx.Provider.ChatCompletionStream(ctx, req, func(se ProviderStreamEvent) error {
			emit(withTurnID(toEvent(se), turnID))
			return nil
		})
		if err != nil {
			emit(Event{Type: EventTypeError, Data: err, TurnID: turnID})
			emit(Event{Type: EventTypeTurnEnd, Data: EventDataTurnEnd{}, TurnID: turnID})
			break AgentLoop
		}

		if len(res.Message.ToolCalls) == 0 && len(res.ToolCalls) > 0 {
			res.Message.ToolCalls = res.ToolCalls
		}
		if res.Message.Role == RoleUnknown || res.Message.Role == "" {
			res.Message.Role = RoleAssistant
		}

		if res.Message.Content != "" || len(res.Message.ToolCalls) > 0 {
			req.Messages = append(req.Messages, res.Message)
		}

		toolCalls := res.ToolCalls
		if len(toolCalls) == 0 {
			toolCalls = res.Message.ToolCalls
		}

		if len(toolCalls) > 0 {
			toolResults := executeTools(ctx, aCtx, turnID, toolCalls, emit)
			req.Messages = append(req.Messages, lo.Map(toolResults, func(r ToolResult, idx int) Message {
				return Message{Role: RoleTool, Content: r.Data, CallID: toolCalls[idx].ID}
			})...)
			shouldContinue = true
		}

		drained := drainSteeringMessages(&req, aCtx, turnID, emit)
		if drained > 0 {
			shouldContinue = true
		}
		if res.Usage.TotalTokens > 0 {
			emit(Event{Type: EventTypeTokenUsage, Data: EventDataTokenUsage{Usage: res.Usage}, TurnID: turnID})
		}
		emit(Event{Type: EventTypeTurnEnd, Data: EventDataTurnEnd{}, TurnID: turnID})
		if aCtx.Steering.HasPending() {
			shouldContinue = true
		}
	}
	emit(Event{Type: EventTypeAgentEnd, Data: EventDataAgentEnd{Messages: req.Messages[1:]}})
}

func drainSteeringMessages(req *ProviderRequest, aCtx Context, turnID int, emit func(Event)) int {
	if req == nil || aCtx.Steering == nil {
		return 0
	}
	messages := aCtx.Steering.Drain()
	if len(messages) == 0 {
		return 0
	}

	now := time.Now().UTC()
	for _, steer := range messages {
		msg := Message{Role: RoleUser, Content: steer.Content}
		req.Messages = append(req.Messages, msg)
		emit(Event{Type: EventTypeMessage, Data: EventDataMessage{Message: msg}, TurnID: turnID, Timestamp: now})
	}
	return len(messages)
}

func executeTools(ctx context.Context, aContext Context, turnID int, toolCalls []ToolCall, onEvent func(Event)) []ToolResult {
	ret := make([]ToolResult, 0, len(toolCalls))
	for _, toolCall := range toolCalls {
		onEvent(Event{Type: EventTypeToolCallStart, Data: EventDataToolCallStart{Call: toolCall}, TurnID: turnID, ToolCallID: toolCall.ID})
		toolName := toolCall.Name
		var (
			result ToolResult
			args   = make(map[string]any)
		)
		tool, ok := aContext.Tools[toolName]
		if !ok {
			result = ToolResult{IsErr: true, Data: "Not found"}
		} else if err := json.Unmarshal([]byte(toolCall.Arguments), &args); err != nil {
			result = ToolResult{IsErr: true, Data: fmt.Sprintf("Invalid arguments: %q", toolCall.Arguments)}
		} else {
			toolCtx := withInteractionRuntime(ctx, interactionRuntime{
				RunID:           aContext.RunID,
				InteractionMgr:  aContext.Interactions,
				EventEmitter:    onEvent,
				CurrentToolCall: toolCall.ID,
				Model:           aContext.Model,
				Provider:        aContext.Provider,
				Approver:        aContext.Approver,
			})
			if aContext.Approver != nil {
				if err := aContext.Approver.Approve(toolCtx, toolName, args); err != nil {
					result = ToolResult{IsErr: true, Data: fmt.Sprintf("permission denied: %v", err)}
				} else {
					result = tool.Handler.Handle(toolCtx, args)
				}
			} else {
				result = tool.Handler.Handle(toolCtx, args)
			}
		}
		onEvent(Event{Type: EventTypeToolCallEnd, Data: EventDataToolCallEnd{Call: toolCall, Result: result}, TurnID: turnID, ToolCallID: toolCall.ID})
		ret = append(ret, result)
	}
	return ret
}

func newEventEmitter(runID string, onEvent func(Event)) func(Event) {
	return func(e Event) {
		if e.RunID == "" {
			e.RunID = runID
		}
		if e.Timestamp.IsZero() {
			e.Timestamp = time.Now().UTC()
		}
		onEvent(e)
	}
}

func withTurnID(event Event, turnID int) Event {
	if event.TurnID == 0 {
		event.TurnID = turnID
	}
	return event
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
