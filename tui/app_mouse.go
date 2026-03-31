package tui

import (
	"image"

	tea "charm.land/bubbletea/v2"
	"github.com/atotto/clipboard"
)

func (m *model) handleMouseMotionMsg(msg tea.MouseMotionMsg, statusCmd tea.Cmd) (tea.Model, tea.Cmd) {
	if m.mouseDown {
		m.mouseDragX = msg.X
		m.mouseDragY = msg.Y
		m.refreshViewport()
	}
	return m, statusCmd
}

func (m *model) handleMouseReleaseMsg(msg tea.MouseReleaseMsg, statusCmd tea.Cmd) (tea.Model, tea.Cmd) {
	if m.mouseDown {
		m.mouseDown = false

		layout := m.computeLayout(m.width, m.height)
		// Extract highlighted content if there is any selection
		if (m.mouseDownX != m.mouseDragX || m.mouseDownY != m.mouseDragY) && layout.valid {
			// Generate the view text
			messageList := m.renderMessageList(layout.mainWidth, layout.messageHeight)
			if m.taskSessionView != nil {
				messageList = m.renderTaskSessionMessageList(layout.mainWidth, layout.messageHeight)
			} else if m.questionDialog != nil {
				messageList = m.renderQuestionDialog(layout.mainWidth, layout.messageHeight)
			} else if m.diffDialog != nil {
				messageList = m.renderDiffDialog(layout.mainWidth, layout.messageHeight)
			} else if m.resumePicker != nil {
				messageList = m.renderResumePickerDialog(layout.mainWidth, layout.messageHeight)
			} else if m.modelPicker != nil {
				messageList = m.renderModelPickerDialog(layout.mainWidth, layout.messageHeight)
			}

			startLine, startCol, endLine, endCol := m.getHighlightRange(layout)
			if startLine >= 0 && endLine >= 0 {
				area := image.Rect(0, 0, layout.mainWidth, layout.messageHeight)
				content := HighlightContent(messageList, area, startLine, startCol, endLine, endCol)
				if content != "" {
					cmd := func() tea.Msg {
						clipboard.WriteAll(content)
						return copyIndicatorMsg{}
					}
					m.refreshViewport()
					return m, tea.Batch(statusCmd, cmd)
				}
			}
		}

		m.refreshViewport()
	}
	return m, statusCmd
}

func (m *model) getHighlightRange(layout layoutState) (startLine, startCol, endLine, endCol int) {
	paneX := appPadding
	paneY := appPadding

	downX := m.mouseDownX - paneX
	downY := m.mouseDownY - paneY
	dragX := m.mouseDragX - paneX
	dragY := m.mouseDragY - paneY

	// If it's a backward selection (dragging up, or dragging left on same line)
	if dragY < downY || (dragY == downY && dragX < downX) {
		startLine, startCol = dragY, dragX
		endLine, endCol = downY, downX
	} else {
		startLine, startCol = downY, downX
		endLine, endCol = dragY, dragX
	}

	// Clamp to message pane dimensions
	if startLine < 0 {
		startLine = 0
		startCol = 0
	}
	if startCol < 0 {
		startCol = 0
	}
	if endLine >= layout.messageHeight {
		endLine = layout.messageHeight - 1
		endCol = layout.mainWidth
	}
	if endCol > layout.mainWidth {
		endCol = layout.mainWidth
	}
	if endLine < 0 || startLine >= layout.messageHeight {
		return -1, -1, -1, -1 // completely out of bounds
	}

	return startLine, startCol, endLine, endCol
}
