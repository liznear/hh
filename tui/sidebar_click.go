package tui

import "github.com/liznear/hh/tui/session"

func (m *model) handleSidebarModifiedFileClick(mouseX int, mouseY int) bool {
	if m == nil {
		return false
	}
	layout := m.computeLayout(m.width, m.height)
	if !layout.valid || !layout.showSidebar {
		return false
	}

	innerX := mouseX - appPadding
	innerY := mouseY - appPadding
	if innerX < 0 || innerY < 0 || innerY >= layout.innerHeight {
		return false
	}

	sidebarStartX := layout.mainWidth + mainSidebarGap
	sidebarEndX := sidebarStartX + layout.sidebarWidth
	if innerX < sidebarStartX || innerX >= sidebarEndX {
		return false
	}

	sidebarContentY := innerY - 1
	if sidebarContentY < 0 {
		return false
	}

	for _, entry := range m.sidebarModifiedFileLines {
		if entry.Line != sidebarContentY {
			continue
		}
		oldContent, newContent, err := gitDiffContentForPath(m.workingDir, entry.Path)
		if err != nil {
			m.addItem(&session.ErrorItem{Message: err.Error()})
			return true
		}
		m.openDiffDialog("Diff: "+displayPath(entry.Path), oldContent, newContent, entry.Path)
		return true
	}

	return false
}
