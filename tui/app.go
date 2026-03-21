package tui

import (
	"context"
	"fmt"
	"strings"

	"github.com/charmbracelet/bubbles/textinput"
	"github.com/charmbracelet/bubbles/viewport"
	tea "github.com/charmbracelet/bubbletea"
	"github.com/charmbracelet/glamour"
	"github.com/charmbracelet/lipgloss"
	"github.com/liznear/hh/agent"
)

type model struct {
	runner    *agent.AgentRunner
	modelName string
	theme     Theme

	width  int
	height int

	input    textinput.Model
	viewport viewport.Model

	stream <-chan tea.Msg
	busy   bool

	lines      []string
	eventCount int

	markdownRenderer      *glamour.TermRenderer
	markdownRendererWidth int
	markdownCache         map[string]string

	activeDeltaType agent.EventType
	activeDeltaLine int
}

type agentStreamStartedMsg struct {
	ch <-chan tea.Msg
}

type agentEventMsg struct {
	event agent.Event
}

type agentRunDoneMsg struct {
	err error
}

func Run(runner *agent.AgentRunner, modelName string) error {
	p := tea.NewProgram(newModel(runner, modelName), tea.WithAltScreen())
	_, err := p.Run()
	return err
}

func newModel(runner *agent.AgentRunner, modelName string) model {
	in := textinput.New()
	in.Prompt = "> "
	in.Placeholder = "Type a prompt and press Enter"
	in.Focus()

	vp := viewport.New(0, 0)

	return model{
		runner:        runner,
		modelName:     modelName,
		theme:         DefaultTheme(),
		input:         in,
		viewport:      vp,
		markdownCache: map[string]string{},
		lines: []string{
			"hh-cli ready",
		},
	}
}

func (m model) Init() tea.Cmd {
	return textinput.Blink
}

func (m model) Update(msg tea.Msg) (tea.Model, tea.Cmd) {
	switch msg := msg.(type) {
	case tea.WindowSizeMsg:
		m.width = msg.Width
		m.height = msg.Height
		m.syncLayout()
		m.refreshViewport()
		return m, nil

	case tea.KeyMsg:
		switch msg.String() {
		case "ctrl+c", "q":
			return m, tea.Quit
		case "enter":
			if m.busy {
				return m, nil
			}
			prompt := strings.TrimSpace(m.input.Value())
			if prompt == "" {
				return m, nil
			}

			m.lines = append(m.lines, fmt.Sprintf("user: %s", prompt))
			m.input.SetValue("")
			m.busy = true
			m.activeDeltaType = ""
			m.activeDeltaLine = -1
			m.refreshViewport()

			return m, startAgentStreamCmd(m.runner, prompt)
		}

		var cmd tea.Cmd
		m.input, cmd = m.input.Update(msg)
		return m, cmd

	case agentStreamStartedMsg:
		m.stream = msg.ch
		return m, waitForStreamCmd(m.stream)

	case agentEventMsg:
		m.eventCount++
		m.handleAgentEvent(msg.event)
		m.refreshViewport()
		return m, waitForStreamCmd(m.stream)

	case agentRunDoneMsg:
		m.busy = false
		if msg.err != nil {
			m.lines = append(m.lines, fmt.Sprintf("error: %v", msg.err))
		}
		m.stream = nil
		m.activeDeltaType = ""
		m.activeDeltaLine = -1
		m.refreshViewport()
		return m, nil
	}

	return m, nil
}

func (m model) View() string {
	if m.width == 0 || m.height == 0 {
		return ""
	}

	m.syncLayout()

	innerW := max(1, m.width-(appPadding*2))
	innerH := max(1, m.height-(appPadding*2))

	showSidebar := m.width > sidebarHideWidth
	mainW := innerW
	if showSidebar {
		mainW = max(1, innerW-sidebarWidth)
	}

	messageH, inputH := computePaneHeights(innerH)

	messagePane := lipgloss.NewStyle().
		Width(mainW).
		Height(messageH).
		Render(m.viewport.View())

	status := "Enter to send, q to quit"
	if m.busy {
		status = "Agent is running..."
	}

	inputBlock := lipgloss.JoinVertical(
		lipgloss.Left,
		lipgloss.NewStyle().Foreground(m.theme.Muted()).Render(status),
		m.input.View(),
	)
	inputPane := lipgloss.NewStyle().
		Width(mainW).
		Height(inputH).
		Render(inputBlock)

	mainPane := lipgloss.NewStyle().
		Width(mainW).
		Height(innerH).
		Render(lipgloss.JoinVertical(lipgloss.Left, messagePane, inputPane))

	content := mainPane
	if showSidebar {
		sidebarText := strings.Join([]string{
			"Session",
			fmt.Sprintf("Model: %s", m.modelName),
			fmt.Sprintf("Status: %s", ternary(m.busy, "running", "idle")),
			fmt.Sprintf("Events: %d", m.eventCount),
		}, "\n")

		sidebarPane := lipgloss.NewStyle().
			Width(sidebarWidth).
			Height(innerH).
			Padding(1).
			Background(m.theme.Surface()).
			Foreground(m.theme.Emphasis()).
			Render(sidebarText)

		content = lipgloss.JoinHorizontal(lipgloss.Top, mainPane, sidebarPane)
	}

	return lipgloss.NewStyle().
		Width(m.width).
		Height(m.height).
		Background(lipgloss.NoColor{}).
		Foreground(lipgloss.NoColor{}).
		Padding(appPadding).
		Render(content)
}

func (m *model) handleAgentEvent(e agent.Event) {
	if e.Type != agent.EventTypeThinkingDelta && e.Type != agent.EventTypeMessageDelta {
		m.activeDeltaType = ""
		m.activeDeltaLine = -1
	}

	switch e.Type {
	case agent.EventTypeThinkingDelta:
		if data, ok := e.Data.(agent.EventDataThinkingDelta); ok {
			m.appendDeltaLine(agent.EventTypeThinkingDelta, "thinking: ", data.Delta)
		}
	case agent.EventTypeMessageDelta:
		data, ok := e.Data.(agent.EventDataMessageDelta)
		if !ok {
			return
		}
		m.appendDeltaLine(agent.EventTypeMessageDelta, "assistant: ", data.Delta)

	case agent.EventTypeToolCallStart:
		if data, ok := e.Data.(agent.EventDataToolCallStart); ok {
			m.lines = append(m.lines, fmt.Sprintf("tool_start: %s", data.Call.Name))
		}
	case agent.EventTypeToolCallEnd:
		if data, ok := e.Data.(agent.EventDataToolCallEnd); ok {
			status := "ok"
			if data.Result.IsErr {
				status = "error"
			}
			m.lines = append(m.lines, fmt.Sprintf("tool_end: %s (%s)", data.Call.Name, status))
		}
	case agent.EventTypeError:
		switch data := e.Data.(type) {
		case error:
			m.lines = append(m.lines, fmt.Sprintf("error: %v", data))
		case agent.EventDataError:
			if data.Err != nil {
				m.lines = append(m.lines, fmt.Sprintf("error: %v", data.Err))
			}
		default:
			m.lines = append(m.lines, "error: unknown")
		}
	}
}

func (m *model) appendDeltaLine(t agent.EventType, prefix, delta string) {
	if delta == "" {
		return
	}

	if m.activeDeltaType == t && m.activeDeltaLine >= 0 && m.activeDeltaLine < len(m.lines) {
		m.lines[m.activeDeltaLine] += delta
		return
	}

	m.lines = append(m.lines, prefix+delta)
	m.activeDeltaType = t
	m.activeDeltaLine = len(m.lines) - 1
}

func (m *model) syncLayout() {
	if m.width == 0 || m.height == 0 {
		return
	}

	innerW := max(1, m.width-(appPadding*2))
	innerH := max(1, m.height-(appPadding*2))
	showSidebar := m.width > sidebarHideWidth

	mainW := innerW
	if showSidebar {
		mainW = max(1, innerW-sidebarWidth)
	}

	messageH, _ := computePaneHeights(innerH)

	m.viewport.Width = mainW
	m.viewport.Height = messageH
	m.input.Width = max(1, mainW-2)
}

func (m *model) refreshViewport() {
	m.viewport.SetContent(m.formatLinesForViewport(m.lines, m.viewport.Width))
	m.viewport.GotoBottom()
}

func (m *model) formatLinesForViewport(lines []string, width int) string {
	if len(lines) == 0 {
		return ""
	}
	if width <= 0 {
		return strings.Join(lines, "\n")
	}

	renderer := m.getMarkdownRenderer(width)

	wrapped := make([]string, 0, len(lines))
	for _, line := range lines {
		if strings.HasPrefix(line, "assistant: ") {
			wrapped = append(wrapped, "assistant:")
			wrapped = append(wrapped, m.renderMarkdown(strings.TrimPrefix(line, "assistant: "), width, renderer))
			continue
		}

		wrapped = append(wrapped, wrapLine(line, width)...)
	}

	return strings.Join(wrapped, "\n")
}

func (m *model) getMarkdownRenderer(width int) *glamour.TermRenderer {
	if m.markdownRenderer != nil && m.markdownRendererWidth == width {
		return m.markdownRenderer
	}

	renderer, err := glamour.NewTermRenderer(
		glamour.WithStandardStyle("light"),
		glamour.WithPreservedNewLines(),
		glamour.WithWordWrap(max(20, width)),
	)
	if err != nil {
		m.markdownRenderer = nil
		m.markdownRendererWidth = 0
		return nil
	}

	m.markdownRenderer = renderer
	m.markdownRendererWidth = width
	m.markdownCache = map[string]string{}
	return m.markdownRenderer
}

func (m *model) renderMarkdown(content string, width int, renderer *glamour.TermRenderer) string {
	if strings.TrimSpace(content) == "" {
		return ""
	}

	cacheKey := fmt.Sprintf("%d:%s", width, content)
	if cached, ok := m.markdownCache[cacheKey]; ok {
		return cached
	}

	if renderer == nil {
		fallback := strings.Join(wrapLine(content, width), "\n")
		m.markdownCache[cacheKey] = fallback
		return fallback
	}

	rendered, err := renderer.Render(content)
	if err != nil {
		fallback := strings.Join(wrapLine(content, width), "\n")
		m.markdownCache[cacheKey] = fallback
		return fallback
	}

	trimmed := strings.TrimRight(rendered, "\n")
	m.markdownCache[cacheKey] = trimmed
	return trimmed
}

func wrapLine(line string, width int) []string {
	if width <= 0 {
		return []string{line}
	}
	if line == "" {
		return []string{""}
	}

	runes := []rune(line)
	ret := make([]string, 0, 1)

	for len(runes) > width {
		breakAt := width
		for i := width; i > 0; i-- {
			if runes[i-1] == ' ' || runes[i-1] == '\t' {
				breakAt = i
				break
			}
		}

		chunk := strings.TrimRight(string(runes[:breakAt]), " \t")
		if chunk == "" {
			breakAt = width
			chunk = string(runes[:breakAt])
		}
		ret = append(ret, chunk)

		runes = runes[breakAt:]
		for len(runes) > 0 && (runes[0] == ' ' || runes[0] == '\t') {
			runes = runes[1:]
		}
	}

	ret = append(ret, string(runes))
	return ret
}

func computePaneHeights(total int) (messageHeight int, inputHeight int) {
	if total <= 2 {
		return 1, 1
	}

	input := defaultInputLines
	if total <= defaultInputLines {
		input = 1
	}

	message := total - input
	if message < 1 {
		message = 1
		input = max(1, total-message)
	}

	return message, input
}

func startAgentStreamCmd(runner *agent.AgentRunner, prompt string) tea.Cmd {
	return func() tea.Msg {
		ch := make(chan tea.Msg)
		go func() {
			err := runner.Run(context.Background(), agent.Input{Content: prompt, Type: "text"}, func(e agent.Event) {
				ch <- agentEventMsg{event: e}
			})
			ch <- agentRunDoneMsg{err: err}
			close(ch)
		}()
		return agentStreamStartedMsg{ch: ch}
	}
}

func waitForStreamCmd(ch <-chan tea.Msg) tea.Cmd {
	return func() tea.Msg {
		if ch == nil {
			return nil
		}
		msg, ok := <-ch
		if !ok {
			return nil
		}
		return msg
	}
}

func ternary[T any](cond bool, ifTrue, ifFalse T) T {
	if cond {
		return ifTrue
	}
	return ifFalse
}
