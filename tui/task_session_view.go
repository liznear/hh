package tui

import (
	"fmt"
	"strings"

	tea "charm.land/bubbletea/v2"
	"github.com/liznear/hh/agent"
	"github.com/liznear/hh/tui/session"
)

type taskSessionViewState struct {
	Session             *session.State
	SubAgentName        string
	Task                string
	TaskIndex           int
	ParentToolCallID    string
	listOffsetIdx       int
	listOffsetLine      int
	autoScroll          bool
	pendingToolByCallID map[string]*session.ToolCallItem
}

func (m *model) openTaskSessionView(target taskLineClickTarget) {
	if target.ParentToolCallID != "" {
		if live := m.getTaskLiveSession(target.ParentToolCallID, target.TaskIndex); live != nil && live.Session != nil {
			m.taskSessionView = &taskSessionViewState{
				Session:             live.Session,
				SubAgentName:        strings.TrimSpace(target.SubAgentName),
				Task:                strings.TrimSpace(target.Task),
				TaskIndex:           target.TaskIndex,
				ParentToolCallID:    strings.TrimSpace(target.ParentToolCallID),
				autoScroll:          true,
				pendingToolByCallID: live.PendingTools,
			}
			return
		}
	}

	state, pending := buildTaskSessionState(target)
	m.taskSessionView = &taskSessionViewState{
		Session:             state,
		SubAgentName:        strings.TrimSpace(target.SubAgentName),
		Task:                strings.TrimSpace(target.Task),
		TaskIndex:           target.TaskIndex,
		ParentToolCallID:    strings.TrimSpace(target.ParentToolCallID),
		autoScroll:          true,
		pendingToolByCallID: pending,
	}
}

func (m *model) closeTaskSessionView() {
	m.taskSessionView = nil
	m.taskLineClickTargets = nil
}

func buildTaskSessionState(target taskLineClickTarget) (*session.State, map[string]*session.ToolCallItem) {
	state := session.NewState("task-session")
	title := strings.TrimSpace(fmt.Sprintf("Task %s: %s", target.SubAgentName, target.Task))
	if title != "" {
		state.SetTitle(title)
	}

	turn := state.StartTurn()
	pending := map[string]*session.ToolCallItem{}
	for _, msg := range target.AgentMessages {
		switch msg.Role {
		case agent.RoleUser:
			turn.AddItem(&session.UserMessage{Content: normalizeTaskSessionMessageContent(msg.Content)})
		case agent.RoleAssistant:
			if len(msg.ToolCalls) == 0 {
				turn.AddItem(&session.AssistantMessage{Content: normalizeTaskSessionMessageContent(msg.Content)})
				continue
			}
			for _, call := range msg.ToolCalls {
				item := &session.ToolCallItem{
					ID:        call.ID,
					Name:      call.Name,
					Arguments: call.Arguments,
					Status:    session.ToolCallStatusPending,
				}
				turn.AddItem(item)
				if strings.TrimSpace(call.ID) != "" {
					pending[call.ID] = item
				}
			}
		case agent.RoleTool:
			content := normalizeTaskSessionMessageContent(msg.Content)
			if item, ok := pending[msg.CallID]; ok {
				item.Status = session.ToolCallStatusSuccess
				item.Result = &session.ToolCallResult{Data: content, Result: content}
				delete(pending, msg.CallID)
				continue
			}
			turn.AddItem(&session.ToolCallItem{
				ID:     msg.CallID,
				Name:   "tool_output",
				Status: session.ToolCallStatusSuccess,
				Result: &session.ToolCallResult{Data: content, Result: content},
			})
		default:
			content := strings.TrimSpace(msg.Content)
			if content != "" {
				turn.AddItem(&session.AssistantMessage{Content: content})
			}
		}
	}
	turn.End()
	return state, pending
}

func normalizeTaskSessionMessageContent(content string) string {
	content = strings.TrimSpace(content)
	if content == "" {
		return "(empty)"
	}
	return content
}

func (m *model) withTaskSessionContext(fn func()) {
	view := m.taskSessionView
	if view == nil || view.Session == nil || fn == nil {
		return
	}

	origSession := m.session
	origIdx := m.listOffsetIdx
	origLine := m.listOffsetLine
	origAutoScroll := m.autoScroll
	origTargets := m.taskLineClickTargets

	m.session = view.Session
	m.listOffsetIdx = view.listOffsetIdx
	m.listOffsetLine = view.listOffsetLine
	m.autoScroll = view.autoScroll
	m.taskLineClickTargets = nil

	fn()

	view.listOffsetIdx = m.listOffsetIdx
	view.listOffsetLine = m.listOffsetLine
	view.autoScroll = m.autoScroll

	m.session = origSession
	m.listOffsetIdx = origIdx
	m.listOffsetLine = origLine
	m.autoScroll = origAutoScroll
	m.taskLineClickTargets = origTargets
}

func (m *model) renderTaskSessionMessageList(width, height int) string {
	if m.taskSessionView == nil {
		return ""
	}
	out := ""
	m.withTaskSessionContext(func() {
		out = m.renderMessageList(width, height)
	})
	m.taskLineClickTargets = nil
	return out
}

func (m *model) handleTaskSessionViewKey(msg tea.KeyPressMsg) bool {
	if m.taskSessionView == nil {
		return false
	}
	if msg.Key().Code == tea.KeyEscape {
		m.closeTaskSessionView()
		return true
	}

	handled := false
	m.withTaskSessionContext(func() {
		handled, _ = m.handleScrollKey(msg)
	})
	if handled {
		return true
	}
	return true
}

func (m *model) handleTaskSessionViewWheel(msg tea.MouseWheelMsg) bool {
	if m.taskSessionView == nil {
		return false
	}
	handled := false
	m.withTaskSessionContext(func() {
		handled = m.handleMouseWheelScroll(msg) != 0
	})
	return handled
}
