package tui

import tea "charm.land/bubbletea/v2"

func (m *model) handleDiffDialogWheel(msg tea.MouseWheelMsg) bool {
	dlg := m.diffDialog
	if dlg == nil {
		return false
	}

	const wheelStep = 3
	switch msg.Mouse().Button {
	case tea.MouseWheelUp:
		dlg.ScrollOffset = max(0, dlg.ScrollOffset-wheelStep)
		return true
	case tea.MouseWheelDown:
		dlg.ScrollOffset += wheelStep
		return true
	default:
		return false
	}
}
