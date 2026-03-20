package agent

type EventType int

const (
	EventTypeError EventType = iota
	EventTypeAgentStart
	EventTypeAgentEnd
	EventTypeTurnStart
	EventTypeTurnEnd
	EventTypeThinkingDelta
	EventTypeMessageDelta
	EventTypeMessage
	EventTypeToolCalls
	EventTypeToolCallStart
	EventTypeToolCallEnd
	EventTypeDone
)

type Event struct {
	Type EventType
	Data any
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

type EventDataDone struct {
	Reason FinishReason
}

type EventStream struct {
	ch <-chan Event
}
