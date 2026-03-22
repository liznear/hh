package tui

import (
	"fmt"
	"strings"
	"time"

	tea "charm.land/bubbletea/v2"
	"github.com/charmbracelet/lipgloss"
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
	return frameViewModel{
		layout:      layout,
		messageList: m.renderMessageList(layout.mainWidth, layout.messageHeight),
		status: statusWidgetModel{
			Busy:          m.busy,
			ShowRunResult: m.showRunResult,
			SpinnerView:   m.spinner.View(),
			Elapsed:       m.stopwatch.Elapsed(),
		},
		sidebarLines: m.buildSidebarLines(),
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
	inputBox := lipgloss.NewStyle().
		Width(layout.inputBoxWidth).
		Height(inputInnerLines).
		Padding(0, 1).
		Border(lipgloss.NormalBorder()).
		BorderForeground(m.theme.Muted()).
		Render(m.input.View())

	inputBlock := lipgloss.JoinVertical(lipgloss.Left, statusLine, inputBox)
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

func (m *model) buildSidebarLines() []string {
	sidebarLines := []string{
		"Session",
		fmt.Sprintf("Model: %s", m.modelName),
		fmt.Sprintf("Status: %s", ternary(m.busy, "running", "idle")),
		fmt.Sprintf("Turns: %d", len(m.session.Turns)),
		fmt.Sprintf("Items: %d", m.session.ItemCount()),
	}
	if !m.debug {
		return sidebarLines
	}

	return append(sidebarLines,
		"",
		"Debug",
		fmt.Sprintf("Render: %s (max %s)", formatDuration(m.lastRenderLatency), formatDuration(m.maxRenderLatency)),
		fmt.Sprintf("Scroll[%s]: %s (max %s, dy=%d)", m.lastScrollStats.inputType, formatDuration(m.lastScrollStats.viewportUpdate), formatDuration(m.maxScrollStats.viewportUpdate), m.lastScrollStats.deltaRows),
		fmt.Sprintf("Scroll gap/view: %s / %s", formatDuration(m.lastScrollStats.updateGap), formatDuration(m.lastScrollStats.timeSinceView)),
		fmt.Sprintf("Scroll gap/view max: %s / %s", formatDuration(m.maxScrollStats.updateGap), formatDuration(m.maxScrollStats.timeSinceView)),
		fmt.Sprintf("Scroll->View: %s / %s", formatDuration(m.lastScrollStats.inputToViewStart), formatDuration(m.lastScrollStats.inputToViewDone)),
		fmt.Sprintf("Scroll->View max: %s / %s (events max %d)", formatDuration(m.maxScrollStats.inputToViewStart), formatDuration(m.maxScrollStats.inputToViewDone), m.maxScrollStats.coalescedEvents),
		fmt.Sprintf("Frame bytes: %d (max %d)", m.lastFrameBytes, m.maxFrameBytes),
	)
}

func (m *model) renderSidebarPane(layout layoutState, sidebarLines []string) string {
	return lipgloss.NewStyle().
		Width(layout.sidebarWidth).
		Height(layout.innerHeight).
		Padding(1).
		Foreground(m.theme.Emphasis()).
		Render(strings.Join(sidebarLines, "\n"))
}

func (m *model) renderSidebarSeparator(layout layoutState) string {
	line := " " + lipgloss.NewStyle().Foreground(m.theme.Muted()).Render("│") + " "
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
