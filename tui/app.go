package tui

import (
	"context"
	"fmt"
	"strings"

	"github.com/charmbracelet/bubbles/textinput"
	"github.com/charmbracelet/bubbles/viewport"
	tea "github.com/charmbracelet/bubbletea"
	"github.com/charmbracelet/lipgloss"
	"github.com/liznear/hh/agent"
)

const (
	sidebarWidth      = 45
	sidebarHideWidth  = 150
	appPadding        = 1
	defaultInputLines = 3
)

type model struct {
	runner    *agent.AgentRunner
	modelName string

	width  int
	height int

	input    textinput.Model
	viewport viewport.Model

	stream <-chan tea.Msg
	busy   bool

	lines      []string
	eventCount int

	assistantStreaming bool
	assistantLineIdx   int
	assistantBuffer    string
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
		runner:    runner,
		modelName: modelName,
		input:     in,
		viewport:  vp,
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
			m.assistantStreaming = false
			m.assistantBuffer = ""
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
		m.assistantStreaming = false
		m.assistantBuffer = ""
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
	m.refreshViewport()

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
		lipgloss.NewStyle().Foreground(lipgloss.Color("241")).Render(status),
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
			Background(lipgloss.Color("236")).
			Foreground(lipgloss.Color("252")).
			Render(sidebarText)

		content = lipgloss.JoinHorizontal(lipgloss.Top, mainPane, sidebarPane)
	}

	return lipgloss.NewStyle().
		Width(m.width).
		Height(m.height).
		Padding(appPadding).
		Render(content)
}

func (m *model) handleAgentEvent(e agent.Event) {
	switch e.Type {
	case agent.EventTypeThinkingDelta:
		if data, ok := e.Data.(agent.EventDataThinkingDelta); ok && strings.TrimSpace(data.Delta) != "" {
			m.lines = append(m.lines, fmt.Sprintf("thinking: %s", data.Delta))
		}
	case agent.EventTypeMessageDelta:
		data, ok := e.Data.(agent.EventDataMessageDelta)
		if !ok {
			return
		}

		if !m.assistantStreaming {
			m.assistantStreaming = true
			m.assistantBuffer = ""
			m.lines = append(m.lines, "assistant: ")
			m.assistantLineIdx = len(m.lines) - 1
		}
		m.assistantBuffer += data.Delta
		m.lines[m.assistantLineIdx] = "assistant: " + m.assistantBuffer

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
	case agent.EventTypeTurnEnd:
		m.assistantStreaming = false
	}
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
	m.viewport.SetContent(strings.Join(m.lines, "\n"))
	m.viewport.GotoBottom()
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
