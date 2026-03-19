package agent

type EventType int

const (
	EventTypeError         = 0
	EventTypeThinkingDelta = 1
)

type Event struct {
	Type EventType
	Data any
}

type EventStream struct {
	ch <-chan Event
}
