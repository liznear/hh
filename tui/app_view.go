package tui

import (
	"fmt"
	"os"
	"strings"
	"time"

	tea "charm.land/bubbletea/v2"
	"github.com/charmbracelet/lipgloss"
	"github.com/liznear/hh/tui/session"
)

type frameViewModel struct {
	layout       layoutState
	messageList  string
	status       statusWidgetModel
	sidebarLines []string
}

func (m *model) View() tea.View {
	start := time.Now()
	scrollPaintPending := !m.runtime.pendingScrollAt.IsZero()
	pendingScrollAt := m.runtime.pendingScrollAt
	pendingScrollEvents := m.runtime.pendingScrollEvents
	defer func() {
		m.runtime.lastRenderLatency = time.Since(start)
		if m.runtime.debug {
			m.runtime.maxRenderLatency = maxDuration(m.runtime.maxRenderLatency, m.runtime.lastRenderLatency)
		}
	}()

	if m.runtime.debug && scrollPaintPending {
		m.runtime.lastScrollStats.inputToViewStart = time.Since(pendingScrollAt)
		m.runtime.lastScrollStats.coalescedEvents = pendingScrollEvents
		m.runtime.maxScrollStats.inputToViewStart = maxDuration(m.runtime.maxScrollStats.inputToViewStart, m.runtime.lastScrollStats.inputToViewStart)
		m.runtime.maxScrollStats.coalescedEvents = maxInt(m.runtime.maxScrollStats.coalescedEvents, pendingScrollEvents)
	}

	layout := m.computeLayout(m.width, m.height)
	if !layout.valid {
		return m.newAppView("")
	}
	m.syncLayoutWith(layout)

	vm := m.buildFrameViewModel(layout)
	content := m.renderFrame(vm)
	if m.runtime.debug {
		m.runtime.lastFrameBytes = len(content)
		m.runtime.maxFrameBytes = maxInt(m.runtime.maxFrameBytes, m.runtime.lastFrameBytes)
	}

	v := m.newAppView(content)
	if m.runtime.debug && scrollPaintPending {
		m.runtime.lastScrollStats.inputToViewDone = time.Since(pendingScrollAt)
		m.runtime.maxScrollStats.inputToViewDone = maxDuration(m.runtime.maxScrollStats.inputToViewDone, m.runtime.lastScrollStats.inputToViewDone)
	}
	if scrollPaintPending {
		m.runtime.pendingScrollAt = time.Time{}
		m.runtime.pendingScrollEvents = 0
	}
	m.runtime.lastViewDoneAt = time.Now()
	return v
}

func (m *model) buildFrameViewModel(layout layoutState) frameViewModel {
	messageList := m.renderMessageList(layout.mainWidth, layout.messageHeight)
	if m.modelPicker != nil {
		messageList = m.renderModelPickerDialog(layout.mainWidth, layout.messageHeight)
	}

	return frameViewModel{
		layout:      layout,
		messageList: messageList,
		status: statusWidgetModel{
			AgentName:     m.agentName,
			ModelName:     m.modelName,
			Busy:          m.runtime.busy,
			ShowRunResult: m.runtime.showRunResult,
			SpinnerView:   m.spinner.View(),
			Elapsed:       m.stopwatch.Elapsed(),
			EscPending:    m.runtime.escPending,
			ShellMode:     m.shellModeActive(),
		},
		sidebarLines: m.buildSidebarLines(layout.sidebarWidth),
	}
}

func (m *model) renderFrame(vm frameViewModel) string {
	messagePane := m.renderMessagePane(vm.layout, vm.messageList)
	inputPane := m.renderInputPane(vm.layout, vm.status)
	mainPane := m.renderMainPane(vm.layout, messagePane, inputPane)

	content := mainPane
	if vm.layout.showSidebar {
		separatorPane := m.renderSidebarSeparator(vm.layout)
		sidebarPane := m.renderSidebarPane(vm.layout, vm.sidebarLines)
		content = lipgloss.JoinHorizontal(lipgloss.Top, mainPane, separatorPane, sidebarPane)
	}

	return m.renderRootFrame(vm.layout, content)
}

func (m *model) renderMessagePane(layout layoutState, messageList string) string {
	return lipgloss.NewStyle().
		Width(layout.mainWidth).
		Height(layout.messageHeight).
		Render(messageList)
}

func (m *model) renderInputPane(layout layoutState, status statusWidgetModel) string {
	statusLine := renderStatusWidget(status, m.theme)
	statusBlock := lipgloss.JoinVertical(lipgloss.Left, "", "", statusLine)
	inputBox := lipgloss.NewStyle().
		Width(layout.inputBoxWidth).
		Border(lipgloss.NormalBorder(), true, false, false, false).
		BorderForeground(m.theme.Color(ThemeColorInputBorder)).
		Height(inputInnerLines).
		Render(m.input.View())

	inputBlock := lipgloss.JoinVertical(lipgloss.Left, statusBlock, inputBox)
	return lipgloss.NewStyle().
		Width(layout.mainWidth).
		Height(layout.inputHeight).
		Render(inputBlock)
}

func (m *model) renderMainPane(layout layoutState, messagePane string, inputPane string) string {
	return lipgloss.NewStyle().
		Width(layout.mainWidth).
		Height(layout.innerHeight).
		Render(lipgloss.JoinVertical(lipgloss.Left, messagePane, inputPane))
}

func (m *model) buildSidebarLines(sidebarWidth int) []string {
	m.refreshGitSnapshotMaybe()

	bold := lipgloss.NewStyle().Bold(true)
	warning := lipgloss.NewStyle().Foreground(m.theme.Color(ThemeColorSidebarWarningForeground))
	errorStyle := lipgloss.NewStyle().Foreground(m.theme.Color(ThemeColorSidebarErrorForeground))
	success := lipgloss.NewStyle().Foreground(m.theme.Color(ThemeColorSidebarSuccessForeground))

	title := strings.TrimSpace(m.session.Title)
	if title == "" {
		title = "Untitled Session"
	}
	title = bold.Render(title)

	wdLine := m.runtime.workingDir
	if wdLine == "" {
		wdLine = "."
	}
	wdLine = beautifySidebarPath(wdLine, os.Getenv("HOME"))
	if strings.TrimSpace(m.runtime.gitBranch) != "" {
		wdLine = fmt.Sprintf("%s @ %s", wdLine, m.runtime.gitBranch)
	}

	usedTokens := maxInt(0, m.runtime.contextWindowUsed)
	totalTokens := m.runtime.contextWindowTotal
	if totalTokens <= 0 {
		totalTokens = 1
	}
	percentage := float64(usedTokens) * 100 / float64(totalTokens)
	numberStyle := lipgloss.NewStyle()
	if percentage > 50 {
		numberStyle = errorStyle
	} else if percentage > 30 {
		numberStyle = warning
	}
	contextLine := fmt.Sprintf(
		"%s %s/%s %s%%",
		bold.Render("Context Usage:"),
		numberStyle.Render(fmt.Sprintf("%d", usedTokens)),
		numberStyle.Render(fmt.Sprintf("%d", totalTokens)),
		numberStyle.Render(fmt.Sprintf("%.1f", percentage)),
	)

	doneTodos := 0
	for _, todo := range m.session.TodoItems {
		if todo.Status == session.TodoStatusCompleted {
			doneTodos++
		}
	}

	sidebarLines := []string{
		title,
		"",
		wdLine,
		"",
		contextLine,
	}

	contentWidth := max(1, sidebarWidth-2)
	if len(m.runtime.modifiedFiles) > 0 {
		sidebarLines = append(sidebarLines, "", bold.Render("Modified Files"))
		for _, file := range m.runtime.modifiedFiles {
			sidebarLines = append(sidebarLines, renderModifiedFileLine(contentWidth, file, success, errorStyle))
		}
	}

	if len(m.session.TodoItems) > 0 {
		sidebarLines = append(sidebarLines, "", bold.Render(fmt.Sprintf("TODO (%d / %d)", doneTodos, len(m.session.TodoItems))))
		for _, item := range m.session.TodoItems {
			line := fmt.Sprintf("[ ] %s", item.Content)
			switch item.Status {
			case session.TodoStatusCompleted:
				line = fmt.Sprintf("[%s] %s", success.Render("✓"), item.Content)
			case session.TodoStatusWIP:
				line = warning.Render(line)
			}
			sidebarLines = append(sidebarLines, line)
		}
	}

	if !m.runtime.debug {
		return sidebarLines
	}

	return append(sidebarLines,
		"",
		bold.Render("Debug"),
		fmt.Sprintf("Render: %s (max %s)", formatDuration(m.runtime.lastRenderLatency), formatDuration(m.runtime.maxRenderLatency)),
		fmt.Sprintf("Scroll[%s]: %s (max %s, dy=%d)", m.runtime.lastScrollStats.inputType, formatDuration(m.runtime.lastScrollStats.viewportUpdate), formatDuration(m.runtime.maxScrollStats.viewportUpdate), m.runtime.lastScrollStats.deltaRows),
		fmt.Sprintf("Scroll gap/view: %s / %s", formatDuration(m.runtime.lastScrollStats.updateGap), formatDuration(m.runtime.lastScrollStats.timeSinceView)),
		fmt.Sprintf("Scroll gap/view max: %s / %s", formatDuration(m.runtime.maxScrollStats.updateGap), formatDuration(m.runtime.maxScrollStats.timeSinceView)),
		fmt.Sprintf("Scroll->View: %s / %s", formatDuration(m.runtime.lastScrollStats.inputToViewStart), formatDuration(m.runtime.lastScrollStats.inputToViewDone)),
		fmt.Sprintf("Scroll->View max: %s / %s (events max %d)", formatDuration(m.runtime.maxScrollStats.inputToViewStart), formatDuration(m.runtime.maxScrollStats.inputToViewDone), m.runtime.maxScrollStats.coalescedEvents),
		fmt.Sprintf("Frame bytes: %d (max %d)", m.runtime.lastFrameBytes, m.runtime.maxFrameBytes),
	)
}

func renderModifiedFileLine(contentWidth int, file modifiedFileStat, addStyle lipgloss.Style, delStyle lipgloss.Style) string {
	left := displayPath(file.Path)
	parts := make([]string, 0, 2)
	if file.Added > 0 {
		parts = append(parts, addStyle.Render(fmt.Sprintf("+%d", file.Added)))
	}
	if file.Deleted > 0 {
		parts = append(parts, delStyle.Render(fmt.Sprintf("-%d", file.Deleted)))
	}
	if len(parts) == 0 {
		return left
	}

	right := strings.Join(parts, " ")
	rightWidth := lipgloss.Width(right)
	if contentWidth <= rightWidth+1 {
		return right
	}
	leftWidth := contentWidth - rightWidth - 1
	if lipgloss.Width(left) > leftWidth {
		left = truncateToWidth(left, leftWidth)
	}
	padding := strings.Repeat(" ", max(1, contentWidth-lipgloss.Width(left)-rightWidth))
	return left + padding + right
}

func (m *model) refreshGitSnapshotMaybe() {
	if m == nil {
		return
	}
	if m.runtime.workingDir == "" {
		m.runtime.workingDir = detectWorkingDirectory()
	}
	if m.runtime.lastGitRefreshAt.IsZero() || time.Since(m.runtime.lastGitRefreshAt) >= sidebarGitRefreshInterval {
		m.runtime.gitBranch = detectGitBranch(m.runtime.workingDir)
		m.runtime.modifiedFiles = collectModifiedFiles(m.runtime.workingDir)
		m.runtime.lastGitRefreshAt = time.Now()
	}
}

func (m *model) renderSidebarPane(layout layoutState, sidebarLines []string) string {
	return lipgloss.NewStyle().
		Width(layout.sidebarWidth).
		Height(layout.innerHeight).
		Padding(1).
		Foreground(lipgloss.NoColor{}).
		Render(strings.Join(sidebarLines, "\n"))
}

func (m *model) renderSidebarSeparator(layout layoutState) string {
	line := " " + lipgloss.NewStyle().Foreground(m.theme.Color(ThemeColorSidebarSeparatorForeground)).Render("│") + " "
	sepLines := make([]string, 0, layout.innerHeight)
	for i := 0; i < layout.innerHeight; i++ {
		sepLines = append(sepLines, line)
	}
	return lipgloss.NewStyle().
		Width(mainSidebarGap).
		Height(layout.innerHeight).
		Render(strings.Join(sepLines, "\n"))
}

func (m *model) renderRootFrame(layout layoutState, content string) string {
	return lipgloss.NewStyle().
		Width(layout.outerWidth).
		Height(layout.outerHeight).
		Background(lipgloss.NoColor{}).
		Foreground(lipgloss.NoColor{}).
		Padding(appPadding).
		Render(content)
}

func (m *model) newAppView(content string) tea.View {
	v := tea.NewView(content)
	v.AltScreen = true
	v.MouseMode = tea.MouseModeCellMotion
	return v
}
