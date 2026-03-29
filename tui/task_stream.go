package tui

import (
	"strings"

	"github.com/liznear/hh/agent"
	"github.com/liznear/hh/tui/session"
)

func (m *model) ensureTaskLiveSession(parentToolCallID string, taskIndex int, subAgentName, task string) *taskSessionLiveState {
	if m.taskLiveSessions == nil {
		m.taskLiveSessions = map[string]map[int]*taskSessionLiveState{}
	}
	parentID := strings.TrimSpace(parentToolCallID)
	if parentID == "" {
		return nil
	}
	byTask, ok := m.taskLiveSessions[parentID]
	if !ok {
		byTask = map[int]*taskSessionLiveState{}
		m.taskLiveSessions[parentID] = byTask
	}
	live, ok := byTask[taskIndex]
	if ok && live != nil {
		return live
	}
	state := session.NewState("task-session")
	state.SetTitle("Task " + strings.TrimSpace(subAgentName) + ": " + strings.TrimSpace(task))
	state.StartTurn()
	live = &taskSessionLiveState{
		SubAgentName: strings.TrimSpace(subAgentName),
		Task:         strings.TrimSpace(task),
		Session:      state,
		PendingTools: map[string]*session.ToolCallItem{},
		Running:      true,
	}
	byTask[taskIndex] = live
	return live
}

func (m *model) getTaskLiveSession(parentToolCallID string, taskIndex int) *taskSessionLiveState {
	if m.taskLiveSessions == nil {
		return nil
	}
	byTask, ok := m.taskLiveSessions[strings.TrimSpace(parentToolCallID)]
	if !ok {
		return nil
	}
	return byTask[taskIndex]
}

func (m *model) handleTaskProgressEvent(data agent.EventDataTaskProgress) {
	live := m.ensureTaskLiveSession(data.ParentToolCallID, data.TaskIndex, data.SubAgentName, data.Task)
	if live == nil || live.Session == nil {
		return
	}
	turn := live.Session.CurrentTurn()
	if turn == nil {
		turn = live.Session.StartTurn()
	}

	subEvent := data.SubEvent
	switch subEvent.Type {
	case agent.EventTypeMessage:
		eventData, ok := subEvent.Data.(agent.EventDataMessage)
		if !ok {
			break
		}
		switch eventData.Message.Role {
		case agent.RoleUser:
			turn.AddItem(&session.UserMessage{Content: normalizeTaskSessionMessageContent(eventData.Message.Content)})
		case agent.RoleAssistant:
			if len(eventData.Message.ToolCalls) == 0 {
				turn.AddItem(&session.AssistantMessage{Content: normalizeTaskSessionMessageContent(eventData.Message.Content)})
				break
			}
			for _, call := range eventData.Message.ToolCalls {
				item := &session.ToolCallItem{ID: call.ID, Name: call.Name, Arguments: call.Arguments, Status: session.ToolCallStatusPending}
				turn.AddItem(item)
				if strings.TrimSpace(call.ID) != "" {
					live.PendingTools[call.ID] = item
				}
			}
		case agent.RoleTool:
			content := normalizeTaskSessionMessageContent(eventData.Message.Content)
			if item, ok := live.PendingTools[eventData.Message.CallID]; ok {
				item.Status = session.ToolCallStatusSuccess
				item.Result = &session.ToolCallResult{Data: content, Result: content}
				delete(live.PendingTools, eventData.Message.CallID)
				break
			}
			turn.AddItem(&session.ToolCallItem{ID: eventData.Message.CallID, Name: "tool_output", Status: session.ToolCallStatusSuccess, Result: &session.ToolCallResult{Data: content, Result: content}})
		}
	case agent.EventTypeMessageDelta:
		eventData, ok := subEvent.Data.(agent.EventDataMessageDelta)
		if !ok {
			break
		}
		last := turn.LastItem()
		if msg, ok := last.(*session.AssistantMessage); ok {
			msg.Append(eventData.Delta)
		} else {
			turn.AddItem(&session.AssistantMessage{Content: eventData.Delta})
		}
	case agent.EventTypeThinkingDelta:
		eventData, ok := subEvent.Data.(agent.EventDataThinkingDelta)
		if !ok {
			break
		}
		last := turn.LastItem()
		if block, ok := last.(*session.ThinkingBlock); ok {
			block.Append(eventData.Delta)
		} else {
			turn.AddItem(&session.ThinkingBlock{Content: eventData.Delta})
		}
	case agent.EventTypeToolCallStart:
		eventData, ok := subEvent.Data.(agent.EventDataToolCallStart)
		if !ok {
			break
		}
		item := &session.ToolCallItem{ID: eventData.Call.ID, Name: eventData.Call.Name, Arguments: eventData.Call.Arguments, Status: session.ToolCallStatusPending}
		turn.AddItem(item)
		if strings.TrimSpace(eventData.Call.ID) != "" {
			live.PendingTools[eventData.Call.ID] = item
		}
	case agent.EventTypeToolCallEnd:
		eventData, ok := subEvent.Data.(agent.EventDataToolCallEnd)
		if !ok {
			break
		}
		if item, ok := live.PendingTools[eventData.Call.ID]; ok {
			item.Complete(eventData.Result)
			delete(live.PendingTools, eventData.Call.ID)
		} else {
			item := &session.ToolCallItem{ID: eventData.Call.ID, Name: eventData.Call.Name, Arguments: eventData.Call.Arguments}
			item.Complete(eventData.Result)
			turn.AddItem(item)
		}
	case agent.EventTypeAgentEnd:
		turn.End()
		live.Running = false
	case agent.EventTypeError:
		switch errData := subEvent.Data.(type) {
		case error:
			turn.AddItem(&session.ErrorItem{Message: errData.Error()})
		case agent.EventDataError:
			if errData.Err != nil {
				turn.AddItem(&session.ErrorItem{Message: errData.Err.Error()})
			}
		}
	}

	if view := m.taskSessionView; view != nil && view.ParentToolCallID == data.ParentToolCallID && view.TaskIndex == data.TaskIndex {
		view.Session = live.Session
	}
	m.refreshAfterStreamEvent()
}
