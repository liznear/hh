package tui

import (
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"os"
	"time"

	"github.com/liznear/hh/agent"
	"github.com/liznear/hh/tools"
	"github.com/liznear/hh/tui/session"
)

func (m *model) handleAgentEvent(e agent.Event) {
	m.maybeClearQueuedSteering(e)

	switch e.Type {
	case agent.EventTypeTurnStart:
	case agent.EventTypeThinkingDelta:
		if data, ok := e.Data.(agent.EventDataThinkingDelta); ok {
			m.appendThinkingDelta(data.Delta)
		}
	case agent.EventTypeMessageDelta:
		data, ok := e.Data.(agent.EventDataMessageDelta)
		if !ok {
			return
		}
		m.appendMessageDelta(data.Delta)
	case agent.EventTypeMessage:
		data, ok := e.Data.(agent.EventDataMessage)
		if !ok {
			return
		}
		if data.Message.Role == agent.RoleUser {
			m.addItem(&session.UserMessage{Content: data.Message.Content})
		}

	case agent.EventTypeToolCallStart:
		if data, ok := e.Data.(agent.EventDataToolCallStart); ok {
			m.addToolCall(data.Call)
		}
	case agent.EventTypeToolCallEnd:
		if data, ok := e.Data.(agent.EventDataToolCallEnd); ok {
			m.completeToolCall(data.Call, data.Result)
		}
	case agent.EventTypeError:
		switch data := e.Data.(type) {
		case error:
			if errors.Is(data, context.Canceled) && m.runtime.cancelledRun {
				return
			}
			m.addItem(&session.ErrorItem{Message: data.Error()})
		case agent.EventDataError:
			if data.Err != nil {
				if errors.Is(data.Err, context.Canceled) && m.runtime.cancelledRun {
					return
				}
				m.addItem(&session.ErrorItem{Message: data.Err.Error()})
			}
		default:
			m.addItem(&session.ErrorItem{Message: "unknown error"})
		}
	case agent.EventTypeSessionTitle:
		if data, ok := e.Data.(agent.EventDataSessionTitle); ok {
			m.session.SetTitle(data.Title)
			m.persistMeta()
		}
	case agent.EventTypeTokenUsage:
		if data, ok := e.Data.(agent.EventDataTokenUsage); ok {
			if data.Usage.TotalTokens > 0 {
				m.runtime.contextWindowUsed = data.Usage.TotalTokens
			}
		}
	case agent.EventTypeInteractionRequested:
		if data, ok := e.Data.(agent.EventDataInteractionRequested); ok {
			if data.Request.Kind == agent.InteractionKindQuestion {
				m.openQuestionDialog(data.Request)
			}
		}
	case agent.EventTypeInteractionResponded:
		if data, ok := e.Data.(agent.EventDataInteractionResponded); ok {
			if dlg := m.runtime.questionDialog; dlg != nil && dlg.request.InteractionID == data.Response.InteractionID {
				m.closeQuestionDialog()
			}
		}
	case agent.EventTypeInteractionDismissed:
		if data, ok := e.Data.(agent.EventDataInteractionDismissed); ok {
			if dlg := m.runtime.questionDialog; dlg != nil && dlg.request.InteractionID == data.InteractionID {
				m.closeQuestionDialog()
			}
		}
	case agent.EventTypeInteractionExpired:
		if data, ok := e.Data.(agent.EventDataInteractionExpired); ok {
			if dlg := m.runtime.questionDialog; dlg != nil && dlg.request.InteractionID == data.InteractionID {
				m.closeQuestionDialog()
				m.addItem(&session.ErrorItem{Message: "question timed out"})
			}
		}
	}
}

func (m *model) maybeClearQueuedSteering(e agent.Event) {
	if len(m.runtime.queuedSteering) == 0 {
		return
	}

	switch e.Type {
	case agent.EventTypeTurnStart, agent.EventTypeTurnEnd, agent.EventTypeAgentEnd:
		m.runtime.queuedSteering = nil
		return
	case agent.EventTypeMessage:
		data, ok := e.Data.(agent.EventDataMessage)
		if ok && data.Message.Role == agent.RoleUser {
			m.runtime.queuedSteering = nil
		}
	}
}

func (m *model) appendThinkingDelta(delta string) {
	if delta == "" {
		return
	}
	last := m.session.LastItem()
	if thinking, ok := last.(*session.ThinkingBlock); ok {
		thinking.Append(delta)
		m.persistMeta()
		return
	}
	m.addItem(&session.ThinkingBlock{Content: delta})
}

func (m *model) appendMessageDelta(delta string) {
	if delta == "" {
		return
	}
	last := m.session.LastItem()
	if _, ok := last.(*session.ThinkingBlock); ok {
		items := m.session.CurrentTurnItems()
		for i := len(items) - 2; i >= 0; i-- {
			if msg, ok := items[i].(*session.AssistantMessage); ok {
				msg.Append(delta)
				m.persistMeta()
				return
			}
			break
		}
	}
	if msg, ok := last.(*session.AssistantMessage); ok {
		msg.Append(delta)
		m.persistMeta()
		return
	}
	m.addItem(&session.AssistantMessage{Content: delta})
}

func (m *model) addToolCall(call agent.ToolCall) {
	key := toolCallKey(call)
	item := &session.ToolCallItem{
		ID:        call.ID,
		Name:      call.Name,
		Arguments: call.Arguments,
		Status:    session.ToolCallStatusPending,
	}
	m.toolCalls[key] = item
	m.addItem(item)
}

func (m *model) completeToolCall(call agent.ToolCall, result agent.ToolResult) {
	key := toolCallKey(call)
	if item, ok := m.toolCalls[key]; ok {
		item.Complete(result)
		m.applyToolState(call, result)
		m.runtime.lastGitRefreshAt = time.Time{}
		m.persistItem(m.turnNumber(m.session.CurrentTurn()), item)
		m.persistMeta()
		delete(m.toolCalls, key)
		return
	}

	item := &session.ToolCallItem{
		ID:        call.ID,
		Name:      call.Name,
		Arguments: call.Arguments,
	}
	item.Complete(result)
	m.applyToolState(call, result)
	m.runtime.lastGitRefreshAt = time.Time{}
	m.addItem(item)
}

func (m *model) applyToolState(call agent.ToolCall, result agent.ToolResult) {
	if result.IsErr {
		return
	}
	if call.Name != "todo_write" {
		return
	}
	items, ok := toSessionTodoItems(result.Result)
	if !ok {
		return
	}
	m.session.SetTodoItems(items)
}

func toSessionTodoItems(raw any) ([]session.TodoItem, bool) {
	if raw == nil {
		return nil, true
	}

	decoded, ok := raw.(tools.TodoWriteResult)
	if !ok {
		ptr, ok := raw.(*tools.TodoWriteResult)
		if ok && ptr != nil {
			decoded = *ptr
		} else {
			buf, err := json.Marshal(raw)
			if err != nil {
				return nil, false
			}
			if err := json.Unmarshal(buf, &decoded); err != nil {
				return nil, false
			}
		}
	}

	items := make([]session.TodoItem, 0, len(decoded.TodoItems))
	for _, item := range decoded.TodoItems {
		items = append(items, session.TodoItem{
			Content: item.Content,
			Status:  session.TodoStatus(item.Status),
		})
	}
	return items, true
}

func (m *model) addItem(item session.Item) {
	m.session.AddItem(item)
	m.persistState()
}

func (m *model) addItemToTurn(turn *session.Turn, item session.Item) {
	if turn == nil {
		return
	}
	turn.AddItem(item)
	m.persistState()
}

func (m *model) persistTurnStart(_ *session.Turn) {
	m.persistState()
}

func (m *model) persistTurnEnd(_ *session.Turn) {
	m.persistState()
}

func (m *model) persistMeta() {
	m.persistState()
}

func (m *model) persistItem(_ int, _ session.Item) {
	m.persistState()
}

func (m *model) turnNumber(turn *session.Turn) int {
	for i, t := range m.session.Turns {
		if t == turn {
			return i + 1
		}
	}
	return 0
}

func (m *model) persistState() {
	if m.storage == nil {
		return
	}
	if !m.hasSubmittedUserPrompt() {
		return
	}
	if err := m.storage.Save(m.session); err != nil {
		fmt.Fprintf(os.Stderr, "failed to persist session: %v\n", err)
	}
}

func (m *model) hasSubmittedUserPrompt() bool {
	if m.session == nil {
		return false
	}
	for _, item := range m.session.AllItems() {
		if _, ok := item.(*session.UserMessage); ok {
			return true
		}
	}
	return false
}
