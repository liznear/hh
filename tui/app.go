package tui

import (
	"context"
	"fmt"
	"os"
	"strconv"
	"strings"
	"time"

	"charm.land/bubbles/v2/spinner"
	"charm.land/bubbles/v2/stopwatch"
	"charm.land/bubbles/v2/textarea"
	"charm.land/bubbles/v2/viewport"
	tea "charm.land/bubbletea/v2"
	"github.com/charmbracelet/glamour"
	"github.com/charmbracelet/lipgloss"
	"github.com/liznear/hh/agent"
	"github.com/liznear/hh/tui/components"
)

type model struct {
	runner    *agent.AgentRunner
	modelName string
	theme     Theme

	width  int
	height int

	input    textarea.Model
	viewport viewport.Model

	stream     <-chan tea.Msg
	busy       bool
	autoScroll bool
	debug      bool

	lines      []string
	eventCount int

	lastRenderLatency time.Duration

	markdownRenderer      *glamour.TermRenderer
	markdownRendererWidth int
	markdownCache         map[string]string

	activeDeltaType agent.EventType
	activeDeltaLine int
	toolCallLines   map[string][]int

	spinner       spinner.Model
	stopwatch     stopwatch.Model
	showRunResult bool
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
	p := tea.NewProgram(newModel(runner, modelName))
	_, err := p.Run()
	return err
}

func newModel(runner *agent.AgentRunner, modelName string) *model {
	in := textarea.New()
	in.Prompt = ""
	in.Placeholder = "Type a prompt (Enter to send, Shift+Enter for newline)"
	in.ShowLineNumbers = false
	in.SetHeight(inputInnerLines)
	inputStyles := textarea.DefaultStyles(false)
	inputStyles.Focused.Base = inputStyles.Focused.Base.UnsetBackground()
	inputStyles.Focused.Text = inputStyles.Focused.Text.UnsetBackground()
	inputStyles.Focused.CursorLine = inputStyles.Focused.CursorLine.UnsetBackground()
	inputStyles.Focused.Placeholder = inputStyles.Focused.Placeholder.UnsetBackground()
	inputStyles.Focused.Prompt = inputStyles.Focused.Prompt.UnsetBackground()
	inputStyles.Focused.EndOfBuffer = inputStyles.Focused.EndOfBuffer.UnsetBackground()
	inputStyles.Blurred.Base = inputStyles.Blurred.Base.UnsetBackground()
	inputStyles.Blurred.Text = inputStyles.Blurred.Text.UnsetBackground()
	inputStyles.Blurred.CursorLine = inputStyles.Blurred.CursorLine.UnsetBackground()
	inputStyles.Blurred.Placeholder = inputStyles.Blurred.Placeholder.UnsetBackground()
	inputStyles.Blurred.Prompt = inputStyles.Blurred.Prompt.UnsetBackground()
	inputStyles.Blurred.EndOfBuffer = inputStyles.Blurred.EndOfBuffer.UnsetBackground()
	in.SetStyles(inputStyles)
	in.Focus()

	vp := viewport.New()
	vp.MouseWheelEnabled = true
	theme := DefaultTheme()
	spin := spinner.New(spinner.WithSpinner(spinner.Dot))
	sw := stopwatch.New(stopwatch.WithInterval(time.Second))

	return &model{
		runner:        runner,
		modelName:     modelName,
		theme:         theme,
		input:         in,
		viewport:      vp,
		spinner:       spin,
		stopwatch:     sw,
		autoScroll:    true,
		debug:         isDebugEnabled(),
		markdownCache: map[string]string{},
		toolCallLines: map[string][]int{},
		lines: []string{
			"hh-cli ready",
		},
	}
}

func (m *model) Init() tea.Cmd {
	return textarea.Blink
}

func (m *model) Update(msg tea.Msg) (tea.Model, tea.Cmd) {
	var spinnerCmd tea.Cmd
	if m.busy {
		m.spinner, spinnerCmd = m.spinner.Update(msg)
	}
	var stopwatchCmd tea.Cmd
	m.stopwatch, stopwatchCmd = m.stopwatch.Update(msg)
	statusCmd := tea.Batch(spinnerCmd, stopwatchCmd)

	switch msg := msg.(type) {
	case tea.WindowSizeMsg:
		m.width = msg.Width
		m.height = msg.Height
		m.syncLayout()
		m.refreshViewport()
		return m, statusCmd

	case tea.KeyPressMsg:
		prevOffset := m.viewport.YOffset()
		var viewportCmd tea.Cmd
		m.viewport, viewportCmd = m.viewport.Update(msg)
		if m.viewport.YOffset() != prevOffset {
			m.autoScroll = m.viewport.AtBottom()
			return m, tea.Batch(statusCmd, viewportCmd)
		}

		key := msg.Key()
		if key.Code == tea.KeyEnter {
			if key.Mod&tea.ModShift != 0 {
				m.input.InsertRune('\n')
				return m, statusCmd
			}

			if m.busy {
				return m, statusCmd
			}
			prompt := strings.TrimSpace(m.input.Value())
			if prompt == "" {
				return m, statusCmd
			}

			m.lines = append(m.lines, fmt.Sprintf("user: %s", prompt))
			m.input.SetValue("")
			m.busy = true
			m.showRunResult = false
			m.activeDeltaType = ""
			m.activeDeltaLine = -1
			m.refreshViewport()

			return m, tea.Batch(startAgentStreamCmd(m.runner, prompt), m.stopwatch.Reset(), m.stopwatch.Start(), func() tea.Msg {
				return m.spinner.Tick()
			})
		}

		switch msg.String() {
		case "ctrl+c", "q":
			return m, tea.Quit
		}

		var cmd tea.Cmd
		m.input, cmd = m.input.Update(msg)
		return m, tea.Batch(statusCmd, cmd)

	case tea.MouseWheelMsg:
		prevOffset := m.viewport.YOffset()
		var cmd tea.Cmd
		m.viewport, cmd = m.viewport.Update(msg)
		if m.viewport.YOffset() != prevOffset {
			m.autoScroll = m.viewport.AtBottom()
			return m, tea.Batch(statusCmd, cmd)
		}
		return m, statusCmd

	case spinner.TickMsg:
		if m.busy {
			m.refreshViewport()
		}
		return m, statusCmd

	case agentStreamStartedMsg:
		m.stream = msg.ch
		return m, tea.Batch(statusCmd, waitForStreamCmd(m.stream))

	case agentEventMsg:
		m.eventCount++
		m.handleAgentEvent(msg.event)
		m.refreshViewport()
		return m, tea.Batch(statusCmd, waitForStreamCmd(m.stream))

	case agentRunDoneMsg:
		m.busy = false
		m.stopwatch, _ = m.stopwatch.Update(stopwatch.StartStopMsg{ID: m.stopwatch.ID()})
		m.showRunResult = true
		if msg.err != nil {
			m.lines = append(m.lines, fmt.Sprintf("error: %v", msg.err))
		}
		m.stream = nil
		m.activeDeltaType = ""
		m.activeDeltaLine = -1
		m.refreshViewport()
		return m, statusCmd
	}

	var cmd tea.Cmd
	m.input, cmd = m.input.Update(msg)
	return m, tea.Batch(statusCmd, cmd)
}

func (m *model) View() tea.View {
	start := time.Now()
	defer func() {
		m.lastRenderLatency = time.Since(start)
	}()

	if m.width == 0 || m.height == 0 {
		v := tea.NewView("")
		v.AltScreen = true
		v.MouseMode = tea.MouseModeCellMotion
		return v
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

	status := components.RenderStatusLine(components.StatusLineParams{
		Busy:          m.busy,
		ShowRunResult: m.showRunResult,
		SpinnerView:   m.spinner.View(),
		Elapsed:       m.stopwatch.Elapsed(),
		InfoColor:     m.theme.Info(),
		MutedColor:    m.theme.Muted(),
		SuccessColor:  m.theme.Success(),
	})

	inputBox := lipgloss.NewStyle().
		Width(max(1, mainW-2)).
		Height(inputInnerLines).
		Padding(0, 1).
		Border(lipgloss.NormalBorder()).
		BorderForeground(m.theme.Muted()).
		Render(m.input.View())

	inputBlock := lipgloss.JoinVertical(
		lipgloss.Left,
		status,
		inputBox,
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
		sidebarLines := []string{
			"Session",
			fmt.Sprintf("Model: %s", m.modelName),
			fmt.Sprintf("Status: %s", ternary(m.busy, "running", "idle")),
			fmt.Sprintf("Events: %d", m.eventCount),
		}
		if m.debug {
			sidebarLines = append(sidebarLines,
				"",
				"Debug",
				fmt.Sprintf("Render: %s", formatDuration(m.lastRenderLatency)),
			)
		}
		sidebarText := strings.Join(sidebarLines, "\n")

		sidebarPane := lipgloss.NewStyle().
			Width(sidebarWidth).
			Height(innerH).
			Padding(1).
			Background(m.theme.Surface()).
			Foreground(m.theme.Emphasis()).
			Render(sidebarText)

		content = lipgloss.JoinHorizontal(lipgloss.Top, mainPane, sidebarPane)
	}

	content = lipgloss.NewStyle().
		Width(m.width).
		Height(m.height).
		Background(lipgloss.NoColor{}).
		Foreground(lipgloss.NoColor{}).
		Padding(appPadding).
		Render(content)

	v := tea.NewView(content)
	v.AltScreen = true
	v.MouseMode = tea.MouseModeCellMotion
	return v
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
			key := toolCallKey(data.Call)
			lineIdx := len(m.lines)
			m.toolCallLines[key] = append(m.toolCallLines[key], lineIdx)
			m.lines = append(m.lines, formatPendingToolCallLine(data.Call))
		}
	case agent.EventTypeToolCallEnd:
		if data, ok := e.Data.(agent.EventDataToolCallEnd); ok {
			isErr := data.Result.IsErr

			key := toolCallKey(data.Call)
			lineIndexes := m.toolCallLines[key]
			if len(lineIndexes) == 0 {
				m.lines = append(m.lines, formatCompletedToolCallLine(isErr, data.Call))
				return
			}

			lineIdx := lineIndexes[0]
			m.toolCallLines[key] = lineIndexes[1:]
			if len(m.toolCallLines[key]) == 0 {
				delete(m.toolCallLines, key)
			}

			if lineIdx >= 0 && lineIdx < len(m.lines) {
				m.lines[lineIdx] = formatCompletedToolCallLine(isErr, data.Call)
			} else {
				m.lines = append(m.lines, formatCompletedToolCallLine(isErr, data.Call))
			}
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

	m.viewport.SetWidth(mainW)
	m.viewport.SetHeight(messageH)
	m.input.SetWidth(max(1, mainW-4))
	m.input.SetHeight(inputInnerLines)
}

func (m *model) refreshViewport() {
	prevOffset := m.viewport.YOffset()
	wasAtBottom := m.viewport.AtBottom()
	m.viewport.SetContent(m.formatLinesForViewport(m.lines, m.viewport.Width()))
	if m.autoScroll || wasAtBottom {
		m.viewport.GotoBottom()
		m.autoScroll = true
		return
	}
	m.viewport.SetYOffset(prevOffset)
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

		if strings.HasPrefix(line, toolCallPendingPrefix) {
			body := strings.TrimPrefix(line, toolCallPendingPrefix)
			pending := components.RenderPendingToolCallLine(body, m.spinner.View())
			wrapped = append(wrapped, components.WrapLine(pending, width)...)
			continue
		}

		if strings.HasPrefix(line, toolCallSuccessPrefix) {
			body := strings.TrimPrefix(line, toolCallSuccessPrefix)
			wrapped = append(wrapped, components.RenderCompletedToolCallLine(body, true, width, m.theme.Success(), m.theme.Error())...)
			continue
		}

		if strings.HasPrefix(line, toolCallFailurePrefix) {
			body := strings.TrimPrefix(line, toolCallFailurePrefix)
			wrapped = append(wrapped, components.RenderCompletedToolCallLine(body, false, width, m.theme.Success(), m.theme.Error())...)
			continue
		}

		wrapped = append(wrapped, components.WrapLine(line, width)...)
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
		fallback := strings.Join(components.WrapLine(content, width), "\n")
		m.markdownCache[cacheKey] = fallback
		return fallback
	}

	rendered, err := renderer.Render(content)
	if err != nil {
		fallback := strings.Join(components.WrapLine(content, width), "\n")
		m.markdownCache[cacheKey] = fallback
		return fallback
	}

	trimmed := strings.TrimRight(rendered, "\n")
	m.markdownCache[cacheKey] = trimmed
	return trimmed
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

func isDebugEnabled() bool {
	v := strings.TrimSpace(os.Getenv("HH_DEBUG"))
	enabled, err := strconv.ParseBool(v)
	return err == nil && enabled
}

func formatDuration(d time.Duration) string {
	if d >= time.Millisecond {
		return fmt.Sprintf("%.2fms", float64(d)/float64(time.Millisecond))
	}
	return fmt.Sprintf("%dus", d.Microseconds())
}

const toolCallPendingPrefix = "tool_call_pending: "
const toolCallSuccessPrefix = "tool_call_success: "
const toolCallFailurePrefix = "tool_call_failure: "

func toolCallKey(call agent.ToolCall) string {
	if call.ID != "" {
		return call.ID
	}
	return call.Name + "|" + call.Arguments
}

func formatPendingToolCallLine(call agent.ToolCall) string {
	return toolCallPendingPrefix + formatToolCallBody(call)
}

func formatCompletedToolCallLine(isErr bool, call agent.ToolCall) string {
	body := formatToolCallBody(call)
	if isErr {
		return toolCallFailurePrefix + body
	}
	return toolCallSuccessPrefix + body
}

func formatToolCallBody(call agent.ToolCall) string {
	args := strings.TrimSpace(call.Arguments)
	if args == "" || args == "{}" {
		return call.Name
	}

	const maxArgLen = 120
	runes := []rune(args)
	if len(runes) > maxArgLen {
		args = string(runes[:maxArgLen-1]) + "…"
	}

	return fmt.Sprintf("%s %s", call.Name, args)
}
