package tui

import (
	"strings"
	"time"

	"charm.land/bubbles/v2/spinner"
	"charm.land/bubbles/v2/stopwatch"
	"charm.land/bubbles/v2/textarea"
	tea "charm.land/bubbletea/v2"
	"github.com/charmbracelet/glamour"
	"github.com/liznear/hh/agent"
	"github.com/liznear/hh/tui/session"
)

type model struct {
	runner    *agent.AgentRunner
	modelName string
	theme     Theme
	storage   *session.Storage

	width  int
	height int

	input          textarea.Model
	messageWidth   int
	messageHeight  int
	listOffsetIdx  int
	listOffsetLine int

	stream     <-chan tea.Msg
	busy       bool
	autoScroll bool
	debug      bool

	session   *session.State
	toolCalls map[string]*session.ToolCallItem

	spinner              spinner.Model
	stopwatch            stopwatch.Model
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

	lastRenderLatency     time.Duration
	maxRenderLatency      time.Duration
	lastScrollStats       scrollPerfStats
	maxScrollStats        scrollPerfStats
	markdownRenderer      *glamour.TermRenderer
	markdownRendererWidth int
	markdownCache         map[string]string
	itemRenderCache       map[uintptr]itemRenderCacheEntry
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

func Run(runner *agent.AgentRunner, modelName string) error {
	p := tea.NewProgram(newModel(runner, modelName))
	_, err := p.Run()
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
		runner:          runner,
		modelName:       modelName,
		theme:           theme,
		storage:         store,
		input:           in,
		spinner:         spin,
		stopwatch:       sw,
		autoScroll:      true,
		debug:           isDebugEnabled(),
		markdownCache:   map[string]string{},
		itemRenderCache: map[uintptr]itemRenderCacheEntry{},
		session:         state,
		toolCalls:       map[string]*session.ToolCallItem{},
	}
}

func (m *model) Init() tea.Cmd {
	return textarea.Blink
}

func (m *model) Update(msg tea.Msg) (tea.Model, tea.Cmd) {
	updateStart := time.Now()
	updateGap := time.Duration(0)
	timeSinceView := time.Duration(0)
	if !m.lastUpdateAt.IsZero() {
		updateGap = updateStart.Sub(m.lastUpdateAt)
	}
	if !m.lastViewDoneAt.IsZero() {
		timeSinceView = updateStart.Sub(m.lastViewDoneAt)
	}
	defer func() {
		m.lastUpdateAt = updateStart
	}()

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

			turn := m.session.StartTurn()
			m.persistTurnStart(turn)
			m.addItemToTurn(turn, &session.UserMessage{Content: prompt})
			m.input.SetValue("")
			m.busy = true
			m.showRunResult = false
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
		scrollUpdateStart := time.Now()
		deltaRows := m.handleMouseWheelScroll(msg)
		if deltaRows != 0 {
			m.recordScrollInteraction("mouse", scrollUpdateStart, deltaRows, updateGap, timeSinceView)
			return m, statusCmd
		}
		return m, statusCmd

	case spinner.TickMsg:
		if m.busy && m.hasPendingToolCalls() && (m.autoScroll || m.isListAtBottom(m.messageWidth, m.messageHeight)) && m.shouldRefreshNow() {
			m.refreshViewport()
			m.lastRefreshAt = time.Now()
		} else if m.busy && m.hasPendingToolCalls() {
			m.viewportDirty = true
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
