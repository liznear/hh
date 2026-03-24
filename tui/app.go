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
	"github.com/liznear/hh/config"
	"github.com/liznear/hh/tui/commands"
	"github.com/liznear/hh/tui/session"
)

type model struct {
	runner    *agent.AgentRunner
	agentName string
	modelName string
	theme     Theme
	storage   *session.Storage

	State

	config config.Config

	spinner               spinner.Model
	stopwatch             stopwatch.Model
	markdownRenderer      *glamour.TermRenderer
	markdownRendererWidth int
	markdownCache         map[string]string
	itemRenderCache       map[uintptr]itemRenderCacheEntry
}

type State struct {
	domainState
	uiState
	runtimeState
}

type domainState struct {
	session   *session.State
	toolCalls map[string]*session.ToolCallItem

	slashCommands map[string]commands.Command
}

type uiState struct {
	modelPicker *modelPickerState

	width  int
	height int

	input          textarea.Model
	messageWidth   int
	messageHeight  int
	listOffsetIdx  int
	listOffsetLine int

	stream <-chan tea.Msg
}

// runtimeState holds ephemeral TUI runtime fields that should not be persisted
// as part of session state.
type runtimeState struct {
	busy         bool
	autoScroll   bool
	debug        bool
	shellMode    bool
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

	questionDialog *questionDialogState

	lastRenderLatency time.Duration
	maxRenderLatency  time.Duration
	lastScrollStats   scrollPerfStats
	maxScrollStats    scrollPerfStats

	workingDir        string
	gitBranch         string
	modifiedFiles     []modifiedFileStat
	lastGitRefreshAt  time.Time
	contextWindowUsed int

	questionPromptedAt       time.Time
	questionLastLatency      time.Duration
	questionSubmittedCount   int
	questionValidationErrors int

	queuedSteering []queuedSteeringMessage
}

type queuedSteeringMessage struct {
	Content string
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

type modelPickerState struct {
	index int
}

type markdownPerfStats struct {
	fallbackToWrap bool
}

func newDomainState(state *session.State, modelName string) domainState {
	if state == nil {
		state = session.NewState(modelName)
	}
	return domainState{
		session:       state,
		toolCalls:     map[string]*session.ToolCallItem{},
		slashCommands: commands.BuiltIn(),
	}
}

func newUIState(input textarea.Model) uiState {
	return uiState{input: input}
}

func newRuntimeState(workingDir string) runtimeState {
	return runtimeState{
		autoScroll:       true,
		debug:            isDebugEnabled(),
		workingDir:       workingDir,
		gitBranch:        detectGitBranch(workingDir),
		modifiedFiles:    collectModifiedFiles(workingDir),
		lastGitRefreshAt: time.Now(),
	}
}

func newState(state *session.State, modelName string, input textarea.Model, workingDir string) State {
	return State{
		domainState:  newDomainState(state, modelName),
		uiState:      newUIState(input),
		runtimeState: newRuntimeState(workingDir),
	}
}

func Run(cfg config.Config) error {
	modelName := cfg.DefaultModel()
	agentName := "Build"
	workingDir := detectWorkingDirectory()

	provider, err := cfg.ModelRouterProvider()
	if err != nil {
		return err
	}

	runner, err := newAgentRunner(modelName, provider, agentName, cfg, workingDir)
	if err != nil {
		return err
	}

	p := tea.NewProgram(newModel(runner, modelName, agentName, cfg, workingDir))
	_, err = p.Run()
	return err
}

func newModel(runner *agent.AgentRunner, modelName, agentName string, cfg config.Config, workingDir string) *model {
	in := newTextareaInput()
	theme := DefaultTheme()
	spin := spinner.New(spinner.WithSpinner(spinner.Dot))
	sw := stopwatch.New(stopwatch.WithInterval(time.Second))
	state := session.NewState(modelName)
	store := newSessionStorage(state)

	m := &model{
		runner:          runner,
		agentName:       agentName,
		modelName:       modelName,
		theme:           theme,
		storage:         store,
		config:          cfg,
		spinner:         spin,
		stopwatch:       sw,
		State:           newState(state, modelName, in, workingDir),
		markdownCache:   map[string]string{},
		itemRenderCache: map[uintptr]itemRenderCacheEntry{},
	}
	return m
}

func (m *model) contextWindowTotalFor(modelName string) int {
	if m == nil {
		return 0
	}
	return m.config.ModelContextWindows()[strings.TrimSpace(modelName)]
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
		if m.questionDialog != nil {
			if m.handleQuestionDialogKey(msg) {
				m.refreshViewport()
				return m, statusCmd
			}
		}

		if m.modelPicker != nil {
			if m.handleModelPickerKey(msg) {
				m.showRunResult = false
				m.escPending = false
				return m, statusCmd
			}
		}

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
		if key.Code == tea.KeyEscape && m.busy {
			if m.escPending {
				if m.runCancel != nil {
					m.runCancel()
				}
				m.cancelledRun = true
				m.escPending = false
			} else {
				m.escPending = true
			}
			return m, statusCmd
		}

		if !m.busy {
			if !m.shellMode && msg.String() == "!" && strings.TrimSpace(m.input.Value()) == "" {
				m.setShellMode(true)
				return m, statusCmd
			}

			if m.shellMode && key.Code == tea.KeyBackspace && m.input.Value() == "" {
				m.setShellMode(false)
				return m, statusCmd
			}
		}

		if isInsertNewlineKey(msg) {
			m.input.InsertRune('\n')
			return m, statusCmd
		}

		if key.Code == tea.KeyEnter {

			inputValue := m.input.Value()
			prompt := strings.TrimSpace(inputValue)
			if prompt == "" {
				return m, statusCmd
			}

			if m.busy {
				if m.runner == nil {
					m.addItem(&session.ErrorItem{Message: "runner unavailable"})
					return m, statusCmd
				}
				if err := m.runner.SubmitSteeringMessage(prompt, ""); err != nil {
					m.addItem(&session.ErrorItem{Message: err.Error()})
					return m, statusCmd
				}
				m.queuedSteering = append(m.queuedSteering, queuedSteeringMessage{Content: prompt})
				m.input.SetValue("")
				m.refreshViewport()
				return m, statusCmd
			}

			if m.shellMode {
				command := strings.TrimSpace(inputValue)
				if command == "" {
					return m, statusCmd
				}

				turn := m.session.StartTurn()
				m.persistTurnStart(turn)
				m.input.SetValue("")
				m.setShellMode(false)
				m.busy = true
				m.escPending = false
				m.cancelledRun = false
				m.showRunResult = false
				runCtx, cancel := context.WithCancel(context.Background())
				m.runCancel = cancel
				m.refreshViewport()

				return m, tea.Batch(runShellCommandCmdWithContext(runCtx, command), m.stopwatch.Reset(), m.stopwatch.Start(), func() tea.Msg {
					return m.spinner.Tick()
				})
			}

			if isShellModeInput(inputValue) {
				command := parseShellCommand(inputValue)
				if strings.TrimSpace(command) == "" {
					return m, statusCmd
				}

				turn := m.session.StartTurn()
				m.persistTurnStart(turn)
				m.input.SetValue("")
				m.busy = true
				m.escPending = false
				m.cancelledRun = false
				m.showRunResult = false
				runCtx, cancel := context.WithCancel(context.Background())
				m.runCancel = cancel
				m.refreshViewport()

				return m, tea.Batch(runShellCommandCmdWithContext(runCtx, command), m.stopwatch.Reset(), m.stopwatch.Start(), func() tea.Msg {
					return m.spinner.Tick()
				})
			}

			if m.handleSlashCommand(prompt) {
				m.input.SetValue("")
				m.showRunResult = false
				m.escPending = false
				return m, statusCmd
			}

			turn := m.session.StartTurn()
			m.persistTurnStart(turn)
			submittedPrompt := promptWithInternalState(prompt, m.session.TodoItems)
			m.input.SetValue("")
			m.busy = true
			m.escPending = false
			m.cancelledRun = false
			m.showRunResult = false
			runCtx, cancel := context.WithCancel(context.Background())
			m.runCancel = cancel
			m.refreshViewport()

			return m, tea.Batch(startAgentStreamCmdWithContext(runCtx, m.runner, submittedPrompt), m.stopwatch.Reset(), m.stopwatch.Start(), func() tea.Msg {
				return m.spinner.Tick()
			})
		}

		switch msg.String() {
		case "ctrl+c":
			return m, tea.Quit
		}
		m.escPending = false

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

	case shellCommandDoneMsg:
		if turn := m.session.CurrentTurn(); turn != nil {
			m.addItemToTurn(turn, &session.ShellMessage{Command: msg.command, Output: msg.output})
		}
		m.finalizeRun(msg.err)
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

func isInsertNewlineKey(msg tea.KeyPressMsg) bool {
	key := msg.Key()
	if key.Code == tea.KeyEnter && key.Mod&tea.ModShift != 0 {
		return true
	}

	switch msg.String() {
	case "shift+enter", "ctrl+j":
		return true
	default:
		return false
	}
}
