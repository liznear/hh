package agent

import (
	"errors"
	"strings"
	"sync"
	"time"
)

const maxSteeringMessageLength = 8000

var ErrInvalidSteeringMessage = errors.New("invalid steering message")

type SteeringMessage struct {
	Seq        uint64
	Content    string
	ReceivedAt time.Time
}

type SteeringQueue struct {
	mu      sync.Mutex
	pending []SteeringMessage
	nextSeq uint64
	now     func() time.Time
}

func NewSteeringQueue() *SteeringQueue {
	return &SteeringQueue{
		pending: make([]SteeringMessage, 0, 4),
		now: func() time.Time {
			return time.Now().UTC()
		},
	}
}

func (q *SteeringQueue) Enqueue(content string) (SteeringMessage, error) {
	if q == nil {
		return SteeringMessage{}, ErrNoActiveRun
	}
	trimmed := strings.TrimSpace(content)
	if trimmed == "" || len(trimmed) > maxSteeringMessageLength {
		return SteeringMessage{}, ErrInvalidSteeringMessage
	}

	q.mu.Lock()
	defer q.mu.Unlock()
	q.nextSeq++
	msg := SteeringMessage{Seq: q.nextSeq, Content: trimmed, ReceivedAt: q.now()}
	q.pending = append(q.pending, msg)
	return msg, nil
}

func (q *SteeringQueue) Drain() []SteeringMessage {
	if q == nil {
		return nil
	}
	q.mu.Lock()
	defer q.mu.Unlock()
	if len(q.pending) == 0 {
		return nil
	}
	out := make([]SteeringMessage, len(q.pending))
	copy(out, q.pending)
	q.pending = q.pending[:0]
	return out
}

func (q *SteeringQueue) HasPending() bool {
	if q == nil {
		return false
	}
	q.mu.Lock()
	defer q.mu.Unlock()
	return len(q.pending) > 0
}
