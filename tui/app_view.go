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
	return frameViewModel{
		layout:      layout,
		messageList: m.renderMessageList(layout.mainWidth, layout.messageHeight),
		status: statusWidgetModel{
			Busy:          m.runtime.busy,
			ShowRunResult: m.runtime.showRunResult,
			SpinnerView:   m.spinner.View(),
			Elapsed:       m.stopwatch.Elapsed(),
			EscPending:    m.runtime.escPending,
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
		BorderStyle(lipgloss.NormalBorder()).
		BorderTop(true).
		BorderBottom(true).
		BorderLeft(false).
		BorderRight(false).
		BorderForeground(m.theme.Success()).
		Padding(0, 0, 0, 1).
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
		fmt.Sprintf("Status: %s", ternary(m.runtime.busy, "running", "idle")),
		fmt.Sprintf("Turns: %d", len(m.session.Turns)),
		fmt.Sprintf("Items: %d", m.session.ItemCount()),
	}
	if !m.runtime.debug {
		return sidebarLines
	}

	return append(sidebarLines,
		"",
		"Debug",
		fmt.Sprintf("Render: %s (max %s)", formatDuration(m.runtime.lastRenderLatency), formatDuration(m.runtime.maxRenderLatency)),
		fmt.Sprintf("Scroll[%s]: %s (max %s, dy=%d)", m.runtime.lastScrollStats.inputType, formatDuration(m.runtime.lastScrollStats.viewportUpdate), formatDuration(m.runtime.maxScrollStats.viewportUpdate), m.runtime.lastScrollStats.deltaRows),
		fmt.Sprintf("Scroll gap/view: %s / %s", formatDuration(m.runtime.lastScrollStats.updateGap), formatDuration(m.runtime.lastScrollStats.timeSinceView)),
		fmt.Sprintf("Scroll gap/view max: %s / %s", formatDuration(m.runtime.maxScrollStats.updateGap), formatDuration(m.runtime.maxScrollStats.timeSinceView)),
		fmt.Sprintf("Scroll->View: %s / %s", formatDuration(m.runtime.lastScrollStats.inputToViewStart), formatDuration(m.runtime.lastScrollStats.inputToViewDone)),
		fmt.Sprintf("Scroll->View max: %s / %s (events max %d)", formatDuration(m.runtime.maxScrollStats.inputToViewStart), formatDuration(m.runtime.maxScrollStats.inputToViewDone), m.runtime.maxScrollStats.coalescedEvents),
		fmt.Sprintf("Frame bytes: %d (max %d)", m.runtime.lastFrameBytes, m.runtime.maxFrameBytes),
	)
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
