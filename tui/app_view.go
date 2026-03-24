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
	scrollPaintPending := !m.pendingScrollAt.IsZero()
	pendingScrollAt := m.pendingScrollAt
	pendingScrollEvents := m.pendingScrollEvents
	defer func() {
		m.lastRenderLatency = time.Since(start)
		if m.debug {
			m.maxRenderLatency = maxDuration(m.maxRenderLatency, m.lastRenderLatency)
		}
	}()

	if m.debug && scrollPaintPending {
		m.lastScrollStats.inputToViewStart = time.Since(pendingScrollAt)
		m.lastScrollStats.coalescedEvents = pendingScrollEvents
		m.maxScrollStats.inputToViewStart = maxDuration(m.maxScrollStats.inputToViewStart, m.lastScrollStats.inputToViewStart)
		m.maxScrollStats.coalescedEvents = maxInt(m.maxScrollStats.coalescedEvents, pendingScrollEvents)
	}

	layout := m.computeLayout(m.width, m.height)
	if !layout.valid {
		return m.newAppView("")
	}
	m.syncLayoutWith(layout)

	vm := m.buildFrameViewModel(layout)
	content := m.renderFrame(vm)
	if m.debug {
		m.lastFrameBytes = len(content)
		m.maxFrameBytes = maxInt(m.maxFrameBytes, m.lastFrameBytes)
	}

	v := m.newAppView(content)
	if m.debug && scrollPaintPending {
		m.lastScrollStats.inputToViewDone = time.Since(pendingScrollAt)
		m.maxScrollStats.inputToViewDone = maxDuration(m.maxScrollStats.inputToViewDone, m.lastScrollStats.inputToViewDone)
	}
	if scrollPaintPending {
		m.pendingScrollAt = time.Time{}
		m.pendingScrollEvents = 0
	}
	m.lastViewDoneAt = time.Now()
	return v
}

func (m *model) buildFrameViewModel(layout layoutState) frameViewModel {
	messageList := m.renderMessageList(layout.mainWidth, layout.messageHeight)
	if m.questionDialog != nil {
		messageList = m.renderQuestionDialog(layout.mainWidth, layout.messageHeight)
	} else if m.modelPicker != nil {
		messageList = m.renderModelPickerDialog(layout.mainWidth, layout.messageHeight)
	}

	return frameViewModel{
		layout:      layout,
		messageList: messageList,
		status: statusWidgetModel{
			AgentName:     m.agentName,
			ModelName:     m.modelName,
			Busy:          m.busy,
			ShowRunResult: m.showRunResult,
			SpinnerView:   m.spinner.View(),
			Elapsed:       m.stopwatch.Elapsed(),
			EscPending:    m.escPending,
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

	wdLine := m.workingDir
	if wdLine == "" {
		wdLine = "."
	}
	wdLine = beautifySidebarPath(wdLine, os.Getenv("HOME"))
	if strings.TrimSpace(m.gitBranch) != "" {
		wdLine = fmt.Sprintf("%s @ %s", wdLine, m.gitBranch)
	}

	usedTokens := maxInt(0, m.contextWindowUsed)
	totalTokens := m.contextWindowTotalFor(strings.TrimSpace(m.modelName))
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

	if m.questionSubmittedCount > 0 || m.questionValidationErrors > 0 {
		questionLine := fmt.Sprintf("%s %d", bold.Render("Questions answered:"), m.questionSubmittedCount)
		if m.questionValidationErrors > 0 {
			questionLine += fmt.Sprintf(" (%d errors)", m.questionValidationErrors)
		}
		sidebarLines = append(sidebarLines, "", questionLine)
		if m.questionLastLatency > 0 {
			sidebarLines = append(sidebarLines, fmt.Sprintf("%s %s", bold.Render("Last answer latency:"), formatDuration(m.questionLastLatency)))
		}
	}

	contentWidth := max(1, sidebarWidth-2)
	if len(m.modifiedFiles) > 0 {
		sidebarLines = append(sidebarLines, "", bold.Render("Modified Files"))
		for _, file := range m.modifiedFiles {
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

	if !m.debug {
		return sidebarLines
	}

	return append(sidebarLines,
		"",
		bold.Render("Debug"),
		fmt.Sprintf("Render: %s (max %s)", formatDuration(m.lastRenderLatency), formatDuration(m.maxRenderLatency)),
		fmt.Sprintf("Scroll[%s]: %s (max %s, dy=%d)", m.lastScrollStats.inputType, formatDuration(m.lastScrollStats.viewportUpdate), formatDuration(m.maxScrollStats.viewportUpdate), m.lastScrollStats.deltaRows),
		fmt.Sprintf("Scroll gap/view: %s / %s", formatDuration(m.lastScrollStats.updateGap), formatDuration(m.lastScrollStats.timeSinceView)),
		fmt.Sprintf("Scroll gap/view max: %s / %s", formatDuration(m.maxScrollStats.updateGap), formatDuration(m.maxScrollStats.timeSinceView)),
		fmt.Sprintf("Scroll->View: %s / %s", formatDuration(m.lastScrollStats.inputToViewStart), formatDuration(m.lastScrollStats.inputToViewDone)),
		fmt.Sprintf("Scroll->View max: %s / %s (events max %d)", formatDuration(m.maxScrollStats.inputToViewStart), formatDuration(m.maxScrollStats.inputToViewDone), m.maxScrollStats.coalescedEvents),
		fmt.Sprintf("Frame bytes: %d (max %d)", m.lastFrameBytes, m.maxFrameBytes),
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
	if m.workingDir == "" {
		m.workingDir = detectWorkingDirectory()
	}
	if m.lastGitRefreshAt.IsZero() || time.Since(m.lastGitRefreshAt) >= sidebarGitRefreshInterval {
		m.gitBranch = detectGitBranch(m.workingDir)
		m.modifiedFiles = collectModifiedFiles(m.workingDir)
		m.lastGitRefreshAt = time.Now()
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
