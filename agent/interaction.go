package agent

import (
	"context"
	"errors"
	"fmt"
	"sync"
	"time"
)

type InteractionKind string

const (
	InteractionKindQuestion InteractionKind = "question"
	InteractionKindApproval InteractionKind = "approval"
	InteractionKindConfirm  InteractionKind = "confirm"
)

var (
	ErrUnknownInteraction            = errors.New("unknown interaction")
	ErrDuplicateInteractionReply     = errors.New("duplicate interaction response")
	ErrInteractionDismissed          = errors.New("interaction dismissed")
	ErrInteractionExpired            = errors.New("interaction expired")
	ErrInvalidInteractionRequest     = errors.New("invalid interaction request")
	ErrInvalidInteractionResponse    = errors.New("invalid interaction response")
	ErrNoActiveInteractionSession    = errors.New("no active interaction session")
	ErrNoActiveRun                   = errors.New("no active run")
	ErrInteractionManagerUnavailable = errors.New("interaction manager unavailable")
)

type InteractionOption struct {
	ID          string `json:"id"`
	Title       string `json:"title"`
	Description string `json:"description"`
}

type InteractionRequest struct {
	InteractionID     string              `json:"interaction_id"`
	RunID             string              `json:"run_id,omitempty"`
	ToolCallID        string              `json:"tool_call_id,omitempty"`
	Kind              InteractionKind     `json:"kind"`
	Title             string              `json:"title"`
	Content           string              `json:"content,omitempty"`
	ContentType       string              `json:"content_type,omitempty"`
	Options           []InteractionOption `json:"options"`
	AllowCustomOption bool                `json:"allow_custom_option"`
	Metadata          map[string]any      `json:"metadata,omitempty"`
	CreatedAt         time.Time           `json:"created_at,omitempty"`
	ExpiresAt         *time.Time          `json:"expires_at,omitempty"`
}

type InteractionResponse struct {
	InteractionID    string    `json:"interaction_id"`
	RunID            string    `json:"run_id,omitempty"`
	SelectedOptionID string    `json:"selected_option_id,omitempty"`
	CustomText       string    `json:"custom_text,omitempty"`
	SubmittedAt      time.Time `json:"submitted_at,omitempty"`
}

type pendingInteraction struct {
	req      InteractionRequest
	resultCh chan interactionResult
	resolved bool
}

type interactionResult struct {
	response InteractionResponse
	err      error
}

type InteractionManager struct {
	mu      sync.Mutex
	pending map[string]*pendingInteraction
	closed  map[string]struct{}
	now     func() time.Time
}

func NewInteractionManager() *InteractionManager {
	return &InteractionManager{
		pending: make(map[string]*pendingInteraction),
		closed:  make(map[string]struct{}),
		now: func() time.Time {
			return time.Now().UTC()
		},
	}
}

func (m *InteractionManager) Request(ctx context.Context, req InteractionRequest, onEvent func(Event)) (InteractionResponse, error) {
	if m == nil {
		return InteractionResponse{}, ErrInteractionManagerUnavailable
	}
	if err := validateInteractionRequest(req); err != nil {
		return InteractionResponse{}, err
	}

	if req.CreatedAt.IsZero() {
		req.CreatedAt = m.now()
	}

	pending := &pendingInteraction{
		req:      req,
		resultCh: make(chan interactionResult, 1),
	}

	m.mu.Lock()
	if _, exists := m.pending[req.InteractionID]; exists {
		m.mu.Unlock()
		return InteractionResponse{}, fmt.Errorf("%w: %s", ErrInvalidInteractionRequest, req.InteractionID)
	}
	if _, exists := m.closed[req.InteractionID]; exists {
		m.mu.Unlock()
		return InteractionResponse{}, fmt.Errorf("%w: interaction_id already used", ErrInvalidInteractionRequest)
	}
	m.pending[req.InteractionID] = pending
	m.mu.Unlock()

	if onEvent != nil {
		onEvent(Event{
			Type:          EventTypeInteractionRequested,
			Data:          EventDataInteractionRequested{Request: req},
			RunID:         req.RunID,
			ToolCallID:    req.ToolCallID,
			InteractionID: req.InteractionID,
			Timestamp:     req.CreatedAt,
		})
	}

	var timeout <-chan time.Time
	if req.ExpiresAt != nil {
		duration := req.ExpiresAt.Sub(m.now())
		if duration <= 0 {
			m.removePending(req.InteractionID)
			if onEvent != nil {
				onEvent(Event{
					Type:          EventTypeInteractionExpired,
					Data:          EventDataInteractionExpired{InteractionID: req.InteractionID},
					RunID:         req.RunID,
					ToolCallID:    req.ToolCallID,
					InteractionID: req.InteractionID,
					Timestamp:     m.now(),
				})
			}
			return InteractionResponse{}, ErrInteractionExpired
		}
		timer := time.NewTimer(duration)
		defer timer.Stop()
		timeout = timer.C
	}

	select {
	case result := <-pending.resultCh:
		if result.err != nil {
			if onEvent != nil && errors.Is(result.err, ErrInteractionDismissed) {
				onEvent(Event{
					Type:          EventTypeInteractionDismissed,
					Data:          EventDataInteractionDismissed{InteractionID: req.InteractionID},
					RunID:         req.RunID,
					ToolCallID:    req.ToolCallID,
					InteractionID: req.InteractionID,
					Timestamp:     m.now(),
				})
			}
			return InteractionResponse{}, result.err
		}
		resp := result.response
		if onEvent != nil {
			onEvent(Event{
				Type:          EventTypeInteractionResponded,
				Data:          EventDataInteractionResponded{Response: resp},
				RunID:         req.RunID,
				ToolCallID:    req.ToolCallID,
				InteractionID: req.InteractionID,
				Timestamp:     m.now(),
			})
		}
		return resp, nil
	case <-timeout:
		m.removePending(req.InteractionID)
		if onEvent != nil {
			onEvent(Event{
				Type:          EventTypeInteractionExpired,
				Data:          EventDataInteractionExpired{InteractionID: req.InteractionID},
				RunID:         req.RunID,
				ToolCallID:    req.ToolCallID,
				InteractionID: req.InteractionID,
				Timestamp:     m.now(),
			})
		}
		return InteractionResponse{}, ErrInteractionExpired
	case <-ctx.Done():
		m.removePending(req.InteractionID)
		return InteractionResponse{}, ctx.Err()
	}
}

func (m *InteractionManager) Submit(resp InteractionResponse) error {
	if m == nil {
		return ErrInteractionManagerUnavailable
	}
	if resp.InteractionID == "" {
		return fmt.Errorf("%w: interaction_id is required", ErrInvalidInteractionResponse)
	}

	m.mu.Lock()
	if _, closed := m.closed[resp.InteractionID]; closed {
		m.mu.Unlock()
		return ErrDuplicateInteractionReply
	}
	pending, ok := m.pending[resp.InteractionID]
	if !ok {
		m.mu.Unlock()
		return ErrUnknownInteraction
	}
	if pending.resolved {
		m.mu.Unlock()
		return ErrDuplicateInteractionReply
	}

	if err := validateInteractionResponse(pending.req, resp); err != nil {
		m.mu.Unlock()
		return err
	}

	if resp.SubmittedAt.IsZero() {
		resp.SubmittedAt = m.now()
	}

	pending.resolved = true
	delete(m.pending, resp.InteractionID)
	m.closed[resp.InteractionID] = struct{}{}
	m.mu.Unlock()

	pending.resultCh <- interactionResult{response: resp}
	close(pending.resultCh)
	return nil
}

func (m *InteractionManager) Dismiss(interactionID string) error {
	if m == nil {
		return ErrInteractionManagerUnavailable
	}
	if interactionID == "" {
		return fmt.Errorf("%w: interaction_id is required", ErrInvalidInteractionResponse)
	}

	m.mu.Lock()
	if _, closed := m.closed[interactionID]; closed {
		m.mu.Unlock()
		return ErrDuplicateInteractionReply
	}
	pending, ok := m.pending[interactionID]
	if !ok {
		m.mu.Unlock()
		return ErrUnknownInteraction
	}
	if pending.resolved {
		m.mu.Unlock()
		return ErrDuplicateInteractionReply
	}

	pending.resolved = true
	delete(m.pending, interactionID)
	m.closed[interactionID] = struct{}{}
	m.mu.Unlock()

	pending.resultCh <- interactionResult{err: ErrInteractionDismissed}
	close(pending.resultCh)
	return nil
}

func (m *InteractionManager) removePending(interactionID string) {
	m.mu.Lock()
	defer m.mu.Unlock()
	if _, exists := m.pending[interactionID]; exists {
		delete(m.pending, interactionID)
		m.closed[interactionID] = struct{}{}
	}
}

func validateInteractionRequest(req InteractionRequest) error {
	if req.InteractionID == "" {
		return fmt.Errorf("%w: interaction_id is required", ErrInvalidInteractionRequest)
	}
	if req.Title == "" {
		return fmt.Errorf("%w: title is required", ErrInvalidInteractionRequest)
	}
	if len(req.Options) == 0 {
		return fmt.Errorf("%w: options must not be empty", ErrInvalidInteractionRequest)
	}
	seen := make(map[string]struct{}, len(req.Options))
	for i, option := range req.Options {
		if option.ID == "" {
			return fmt.Errorf("%w: options[%d].id is required", ErrInvalidInteractionRequest, i)
		}
		if option.Title == "" {
			return fmt.Errorf("%w: options[%d].title is required", ErrInvalidInteractionRequest, i)
		}
		if _, exists := seen[option.ID]; exists {
			return fmt.Errorf("%w: duplicate option id %q", ErrInvalidInteractionRequest, option.ID)
		}
		seen[option.ID] = struct{}{}
	}
	return nil
}

func validateInteractionResponse(req InteractionRequest, resp InteractionResponse) error {
	if req.RunID != "" && resp.RunID != "" && req.RunID != resp.RunID {
		return fmt.Errorf("%w: run_id mismatch", ErrInvalidInteractionResponse)
	}
	hasOption := resp.SelectedOptionID != ""
	hasCustom := resp.CustomText != ""
	if hasOption == hasCustom {
		return fmt.Errorf("%w: exactly one of selected_option_id or custom_text is required", ErrInvalidInteractionResponse)
	}
	if hasCustom && !req.AllowCustomOption {
		return fmt.Errorf("%w: custom_text is not allowed", ErrInvalidInteractionResponse)
	}
	if hasOption {
		for _, option := range req.Options {
			if option.ID == resp.SelectedOptionID {
				return nil
			}
		}
		return fmt.Errorf("%w: selected_option_id not found", ErrInvalidInteractionResponse)
	}
	return nil
}

type contextKey string

const interactionRuntimeContextKey contextKey = "agent.interaction_runtime"

type interactionRuntime struct {
	RunID           string
	InteractionMgr  *InteractionManager
	EventEmitter    func(Event)
	CurrentToolCall string
}

func withInteractionRuntime(ctx context.Context, runtime interactionRuntime) context.Context {
	return context.WithValue(ctx, interactionRuntimeContextKey, runtime)
}

func interactionRuntimeFromContext(ctx context.Context) (interactionRuntime, bool) {
	runtime, ok := ctx.Value(interactionRuntimeContextKey).(interactionRuntime)
	return runtime, ok
}

func RequestInteraction(ctx context.Context, req InteractionRequest) (InteractionResponse, error) {
	runtime, ok := interactionRuntimeFromContext(ctx)
	if !ok {
		return InteractionResponse{}, ErrNoActiveInteractionSession
	}
	if runtime.InteractionMgr == nil {
		return InteractionResponse{}, ErrInteractionManagerUnavailable
	}
	if req.RunID == "" {
		req.RunID = runtime.RunID
	}
	if req.ToolCallID == "" {
		req.ToolCallID = runtime.CurrentToolCall
	}
	return runtime.InteractionMgr.Request(ctx, req, runtime.EventEmitter)
}
