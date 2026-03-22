package tui

import (
	"context"
	"strings"
	"time"

	"charm.land/bubbles/v2/spinner"
	"charm.land/bubbles/v2/stopwatch"
	"charm.land/bubbles/v2/textarea"
	tea "charm.land/bubbletea/v2"
	"github.com/charmbracelet/glamour"
	"github.com/liznear/hh/agent"
	"github.com/liznear/hh/tui/commands"
	"github.com/liznear/hh/tui/session"
)

type model struct {
	runner    *agent.AgentRunner
	modelName string
	theme     Theme
	storage   *session.Storage

	slashCommands map[string]commands.Command

	width  int
	height int

	input          textarea.Model
	messageWidth   int
	messageHeight  int
	listOffsetIdx  int
	listOffsetLine int

	stream  <-chan tea.Msg
	runtime RuntimeState

	session   *session.State
	toolCalls map[string]*session.ToolCallItem

	spinner               spinner.Model
	stopwatch             stopwatch.Model
	markdownRenderer      *glamour.TermRenderer
	markdownRendererWidth int
	markdownCache         map[string]string
	itemRenderCache       map[uintptr]itemRenderCacheEntry
}

// RuntimeState holds ephemeral TUI runtime fields that should not be persisted
// as part of session state.
type RuntimeState struct {
	busy         bool
	autoScroll   bool
	debug        bool
	runCancel    context.CancelFunc
	escPending   bool
	cancelledRun bool

	showRunResult        bool
	viewportDirty        bool
	pendingScrollAt      time.Time
	pendingScrollEvents  int
	lastUpdateAt         time.Time
	lastViewDoneAt       time.Time
	lastFrameBytes       int
	maxFrameBytes        int
	lastRefreshAt        time.Time
	suppressRefreshUntil time.Time

	lastRenderLatency time.Duration
	maxRenderLatency  time.Duration
	lastScrollStats   scrollPerfStats
	maxScrollStats    scrollPerfStats
}

type itemRenderCacheEntry struct {
	width     int
	signature string
	lines     []string
}

type scrollPerfStats struct {
	inputType        string
	viewportUpdate   time.Duration
	deltaRows        int
	inputToViewStart time.Duration
	inputToViewDone  time.Duration
	coalescedEvents  int
	updateGap        time.Duration
	timeSinceView    time.Duration
}

type markdownPerfStats struct {
	fallbackToWrap bool
}

func Run(provider agent.Provider, modelName, agentName string) error {
	runner, err := newAgentRunner(modelName, provider, agentName)
	if err != nil {
		return err
	}

	p := tea.NewProgram(newModel(runner, modelName))
	_, err = p.Run()
	return err
}

func newModel(runner *agent.AgentRunner, modelName string) *model {
	in := newTextareaInput()
	theme := DefaultTheme()
	spin := spinner.New(spinner.WithSpinner(spinner.Dot))
	sw := stopwatch.New(stopwatch.WithInterval(time.Second))
	state := session.NewState(modelName)
	store := newSessionStorage(state)

	return &model{
		runner:    runner,
		modelName: modelName,
		theme:     theme,
		storage:   store,
		input:     in,
		spinner:   spin,
		stopwatch: sw,
		runtime: RuntimeState{
			autoScroll: true,
			debug:      isDebugEnabled(),
		},
		markdownCache:   map[string]string{},
		itemRenderCache: map[uintptr]itemRenderCacheEntry{},
		session:         state,
		toolCalls:       map[string]*session.ToolCallItem{},
		slashCommands:   commands.BuiltIn(),
	}
}

func (m *model) Init() tea.Cmd {
	return textarea.Blink
}

func (m *model) Update(msg tea.Msg) (tea.Model, tea.Cmd) {
	updateStart := time.Now()
	updateGap := time.Duration(0)
	timeSinceView := time.Duration(0)
	if !m.runtime.lastUpdateAt.IsZero() {
		updateGap = updateStart.Sub(m.runtime.lastUpdateAt)
	}
	if !m.runtime.lastViewDoneAt.IsZero() {
		timeSinceView = updateStart.Sub(m.runtime.lastViewDoneAt)
	}
	defer func() {
		m.runtime.lastUpdateAt = updateStart
	}()

	var spinnerCmd tea.Cmd
	if m.runtime.busy {
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
		prevOffset := m.currentListOffset(m.messageWidth)
		scrollUpdateStart := time.Now()
		scrolled, deltaRows := m.handleScrollKey(msg)
		if scrolled {
			if deltaRows == 0 {
				deltaRows = m.currentListOffset(m.messageWidth) - prevOffset
			}
			m.recordScrollInteraction("keyboard", scrollUpdateStart, deltaRows, updateGap, timeSinceView)
			return m, statusCmd
		}

		key := msg.Key()
		if key.Code == tea.KeyEscape && m.runtime.busy {
			if m.runtime.escPending {
				if m.runtime.runCancel != nil {
					m.runtime.runCancel()
				}
				m.runtime.cancelledRun = true
				m.runtime.escPending = false
			} else {
				m.runtime.escPending = true
			}
			return m, statusCmd
		}

		if key.Code == tea.KeyEnter {
			if key.Mod&tea.ModShift != 0 {
				m.input.InsertRune('\n')
				return m, statusCmd
			}

			if m.runtime.busy {
				return m, statusCmd
			}
			prompt := strings.TrimSpace(m.input.Value())
			if prompt == "" {
				return m, statusCmd
			}

			if m.handleSlashCommand(prompt) {
				m.input.SetValue("")
				m.runtime.showRunResult = false
				m.runtime.escPending = false
				return m, statusCmd
			}

			turn := m.session.StartTurn()
			m.persistTurnStart(turn)
			m.addItemToTurn(turn, &session.UserMessage{Content: prompt})
			m.input.SetValue("")
			m.runtime.busy = true
			m.runtime.escPending = false
			m.runtime.cancelledRun = false
			m.runtime.showRunResult = false
			runCtx, cancel := context.WithCancel(context.Background())
			m.runtime.runCancel = cancel
			m.refreshViewport()

			return m, tea.Batch(startAgentStreamCmdWithContext(runCtx, m.runner, prompt), m.stopwatch.Reset(), m.stopwatch.Start(), func() tea.Msg {
				return m.spinner.Tick()
			})
		}

		switch msg.String() {
		case "ctrl+c", "q":
			return m, tea.Quit
		}
		m.runtime.escPending = false

		var cmd tea.Cmd
		m.input, cmd = m.input.Update(msg)
		return m, tea.Batch(statusCmd, cmd)

	case tea.MouseWheelMsg:
		scrollUpdateStart := time.Now()
		deltaRows := m.handleMouseWheelScroll(msg)
		if deltaRows != 0 {
			m.recordScrollInteraction("mouse", scrollUpdateStart, deltaRows, updateGap, timeSinceView)
			return m, statusCmd
		}
		return m, statusCmd

	case spinner.TickMsg:
		if m.runtime.busy && m.hasPendingToolCalls() && (m.runtime.autoScroll || m.isListAtBottom(m.messageWidth, m.messageHeight)) && m.shouldRefreshNow() {
			m.refreshViewport()
			m.runtime.lastRefreshAt = time.Now()
		} else if m.runtime.busy && m.hasPendingToolCalls() {
			m.runtime.viewportDirty = true
		}
		return m, statusCmd

	case agentStreamStartedMsg:
		m.stream = msg.ch
		return m, tea.Batch(statusCmd, waitForStreamCmd(m.stream))

	case streamBatchMsg:
		if len(msg.events) > 0 {
			for _, e := range msg.events {
				m.handleAgentEvent(e)
			}
			m.refreshAfterStreamEvent()
		}

		if msg.done {
			m.finalizeRun(msg.doneErr)
			return m, statusCmd
		}

		return m, tea.Batch(statusCmd, waitForStreamCmd(m.stream))

	case agentEventMsg:
		m.handleAgentEvent(msg.event)
		m.refreshAfterStreamEvent()
		return m, tea.Batch(statusCmd, waitForStreamCmd(m.stream))

	case agentRunDoneMsg:
		m.finalizeRun(msg.err)
		return m, statusCmd
	}

	var cmd tea.Cmd
	m.input, cmd = m.input.Update(msg)
	return m, tea.Batch(statusCmd, cmd)
}
