package tui

import (
	"strings"

	"github.com/charmbracelet/x/ansi"
)

func (m *model) handleTaskLineClick(mouseX int, mouseY int) bool {
	if m == nil || m.taskSessionView != nil || m.diffDialog != nil || m.questionDialog != nil || m.modelPicker != nil || m.resumePicker != nil {
		return false
	}
	layout := m.computeLayout(m.width, m.height)
	if !layout.valid {
		return false
	}

	innerX := mouseX - appPadding
	innerY := mouseY - appPadding
	if innerX < 0 || innerY < 0 || innerY >= layout.innerHeight {
		return false
	}
	if innerX >= layout.mainWidth {
		return false
	}

	messageTop := 0
	messageBottom := layout.messageHeight
	if innerY < messageTop || innerY >= messageBottom {
		return false
	}

	viewLine := innerY - messageTop
	for _, target := range m.taskLineClickTargets {
		if target.ViewLine != viewLine {
			continue
		}
		m.openTaskSessionView(target)
		if m.taskSessionView != nil && m.taskSessionView.ParentToolCallID != "" {
			if live := m.getTaskLiveSession(m.taskSessionView.ParentToolCallID, m.taskSessionView.TaskIndex); live == nil {
				m.ensureTaskLiveSession(m.taskSessionView.ParentToolCallID, m.taskSessionView.TaskIndex, m.taskSessionView.SubAgentName, m.taskSessionView.Task)
			}
		}
		return true
	}

	return false
}

func taskLineClickableMarker(line string) bool {
	plain := strings.TrimSpace(ansi.Strip(line))
	return strings.HasPrefix(plain, "• Task ") || strings.HasPrefix(plain, "✓ Task ") || strings.HasPrefix(plain, "⨯ Task ")
}
