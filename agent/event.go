package agent

import "time"

type EventType string

const (
	EventTypeError                EventType = "error"
	EventTypeAgentStart           EventType = "agent_start"
	EventTypeAgentEnd             EventType = "agent_end"
	EventTypeTurnStart            EventType = "turn_start"
	EventTypeTurnEnd              EventType = "turn_end"
	EventTypeThinkingDelta        EventType = "thinking_delta"
	EventTypeMessageDelta         EventType = "message_delta"
	EventTypeMessage              EventType = "message"
	EventTypeToolCalls            EventType = "tool_calls"
	EventTypeToolCallStart        EventType = "tool_call_start"
	EventTypeToolCallEnd          EventType = "tool_call_end"
	EventTypeInteractionRequested EventType = "interaction_requested"
	EventTypeInteractionResponded EventType = "interaction_responded"
	EventTypeInteractionDismissed EventType = "interaction_dismissed"
	EventTypeInteractionExpired   EventType = "interaction_expired"
	EventTypeTokenUsage           EventType = "token_usage"
	EventTypeSessionTitle         EventType = "session_title"
	EventTypeDone                 EventType = "done"
)

type Event struct {
	Type          EventType
	Data          any
	RunID         string
	TurnID        int
	ToolCallID    string
	InteractionID string
	Timestamp     time.Time
}

type EventDataError struct {
	Err error
}

type EventDataAgentStart struct{}

type EventDataAgentEnd struct {
	Messages []Message
}

type EventDataTurnStart struct{}

type EventDataTurnEnd struct{}

type EventDataThinkingDelta struct {
	Delta string
}

type EventDataMessageDelta struct {
	Delta string
}

type EventDataMessage struct {
	Message Message
}

type EventDataToolCalls struct {
	ToolCalls []ToolCall
}

type EventDataToolCallStart struct {
	Call ToolCall
}

type EventDataToolCallEnd struct {
	Call   ToolCall
	Result ToolResult
}

type EventDataInteractionRequested struct {
	Request InteractionRequest
}

type EventDataInteractionResponded struct {
	Response InteractionResponse
}

type EventDataInteractionDismissed struct {
	InteractionID string
}

type EventDataInteractionExpired struct {
	InteractionID string
}

type EventDataTokenUsage struct {
	Usage TokenUsage
}

type EventDataSessionTitle struct {
	Title string
}

type EventDataDone struct {
	Reason FinishReason
}

type EventStream struct {
	ch <-chan Event
}
