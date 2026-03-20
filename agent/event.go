package agent

type EventType int

const (
	EventTypeError EventType = iota
	EventTypeThinkingDelta
	EventTypeMessageDelta
	EventTypeMessage
	EventTypeToolCalls
	EventTypeDone
)

type Event struct {
	Type EventType
	Data any
}

type EventDataError struct {
	Err error
}

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

type EventDataDone struct {
	Reason FinishReason
}

type EventStream struct {
	ch <-chan Event
}
