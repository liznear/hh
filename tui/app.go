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
	"github.com/liznear/hh/tui/session"
)

type model struct {
	runner    *agent.AgentRunner
	modelName string
	theme     Theme
	storage   *session.Storage

	width  int
	height int

	input    textarea.Model
	viewport viewport.Model

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
	lastLoopStats        loopPerfStats
	maxLoopStats         loopPerfStats
	lastFrameBytes       int
	maxFrameBytes        int
	lastRefreshAt        time.Time
	pendingEventRefresh  int
	suppressRefreshUntil time.Time

	lastRenderLatency     time.Duration
	maxRenderLatency      time.Duration
	lastFormatStats       formatPerfStats
	maxFormatStats        formatPerfStats
	lastViewStats         viewPerfStats
	maxViewStats          viewPerfStats
	lastScrollStats       scrollPerfStats
	maxScrollStats        scrollPerfStats
	markdownRenderer      *glamour.TermRenderer
	markdownRendererWidth int
	markdownCache         map[string]string
	lastViewportContent   string
}

type formatPerfStats struct {
	lineCount          int
	assistantLineCount int
	formatDuration     time.Duration
	setContentDuration time.Duration
	refreshDuration    time.Duration
	wrapDuration       time.Duration
	markdownDuration   time.Duration
	markdownCalls      int
	markdownCacheHits  int
	markdownFallbacks  int
}

type markdownPerfStats struct {
	calls          int
	cacheHits      int
	renderDuration time.Duration
	fallbackToWrap bool
}

type viewPerfStats struct {
	viewportViewDuration time.Duration
	statusDuration       time.Duration
	inputDuration        time.Duration
	layoutDuration       time.Duration
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

type loopPerfStats struct {
	updateGap      time.Duration
	updateDuration time.Duration
	timeSinceView  time.Duration
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

type streamBatchMsg struct {
	events  []agent.Event
	done    bool
	doneErr error
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
	state := session.NewState(modelName)

	var store *session.Storage
	if dir, err := session.DefaultStorageDir(); err == nil {
		if s, err := session.NewStorage(dir); err == nil {
			store = s
			if err := store.SaveMeta(state); err != nil {
				fmt.Fprintf(os.Stderr, "failed to save session metadata: %v\n", err)
			}
		} else {
			fmt.Fprintf(os.Stderr, "failed to initialize session storage: %v\n", err)
		}
	} else {
		fmt.Fprintf(os.Stderr, "failed to resolve session storage directory: %v\n", err)
	}

	return &model{
		runner:        runner,
		modelName:     modelName,
		theme:         theme,
		storage:       store,
		input:         in,
		viewport:      vp,
		spinner:       spin,
		stopwatch:     sw,
		autoScroll:    true,
		debug:         isDebugEnabled(),
		markdownCache: map[string]string{},
		session:       state,
		toolCalls:     map[string]*session.ToolCallItem{},
	}
}

func (m *model) Init() tea.Cmd {
	return textarea.Blink
}

func (m *model) Update(msg tea.Msg) (tea.Model, tea.Cmd) {
	updateStart := time.Now()
	updateGap := time.Duration(0)
	timeSinceView := time.Duration(0)
	if m.debug {
		if !m.lastUpdateAt.IsZero() {
			updateGap = updateStart.Sub(m.lastUpdateAt)
		}
		if !m.lastViewDoneAt.IsZero() {
			timeSinceView = updateStart.Sub(m.lastViewDoneAt)
		}
		m.lastLoopStats.updateGap = updateGap
		m.lastLoopStats.timeSinceView = timeSinceView
		m.maxLoopStats.updateGap = maxDuration(m.maxLoopStats.updateGap, updateGap)
		m.maxLoopStats.timeSinceView = maxDuration(m.maxLoopStats.timeSinceView, timeSinceView)
	}
	defer func() {
		if m.debug {
			d := time.Since(updateStart)
			m.lastLoopStats.updateDuration = d
			m.maxLoopStats.updateDuration = maxDuration(m.maxLoopStats.updateDuration, d)
		}
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
		prevOffset := m.viewport.YOffset()
		var viewportCmd tea.Cmd
		viewportUpdateStart := time.Now()
		m.viewport, viewportCmd = m.viewport.Update(msg)
		if m.viewport.YOffset() != prevOffset {
			m.autoScroll = m.viewport.AtBottom()
			m.suppressRefreshUntil = time.Now().Add(scrollPriorityWindow)
			if m.pendingScrollAt.IsZero() {
				m.pendingScrollAt = time.Now()
				m.pendingScrollEvents = 0
			}
			m.pendingScrollEvents++
			if m.autoScroll && m.viewportDirty {
				m.refreshViewport()
				m.viewportDirty = false
			}
			if m.debug {
				m.lastScrollStats = scrollPerfStats{
					inputType:      "keyboard",
					viewportUpdate: time.Since(viewportUpdateStart),
					deltaRows:      m.viewport.YOffset() - prevOffset,
					updateGap:      updateGap,
					timeSinceView:  timeSinceView,
				}
				m.maxScrollStats.viewportUpdate = maxDuration(m.maxScrollStats.viewportUpdate, m.lastScrollStats.viewportUpdate)
				m.maxScrollStats.updateGap = maxDuration(m.maxScrollStats.updateGap, m.lastScrollStats.updateGap)
				m.maxScrollStats.timeSinceView = maxDuration(m.maxScrollStats.timeSinceView, m.lastScrollStats.timeSinceView)
			}
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
		prevOffset := m.viewport.YOffset()
		var cmd tea.Cmd
		viewportUpdateStart := time.Now()
		m.viewport, cmd = m.viewport.Update(msg)
		if m.viewport.YOffset() != prevOffset {
			m.autoScroll = m.viewport.AtBottom()
			m.suppressRefreshUntil = time.Now().Add(scrollPriorityWindow)
			if m.pendingScrollAt.IsZero() {
				m.pendingScrollAt = time.Now()
				m.pendingScrollEvents = 0
			}
			m.pendingScrollEvents++
			if m.autoScroll && m.viewportDirty {
				m.refreshViewport()
				m.viewportDirty = false
			}
			if m.debug {
				m.lastScrollStats = scrollPerfStats{
					inputType:      "mouse",
					viewportUpdate: time.Since(viewportUpdateStart),
					deltaRows:      m.viewport.YOffset() - prevOffset,
					updateGap:      updateGap,
					timeSinceView:  timeSinceView,
				}
				m.maxScrollStats.viewportUpdate = maxDuration(m.maxScrollStats.viewportUpdate, m.lastScrollStats.viewportUpdate)
				m.maxScrollStats.updateGap = maxDuration(m.maxScrollStats.updateGap, m.lastScrollStats.updateGap)
				m.maxScrollStats.timeSinceView = maxDuration(m.maxScrollStats.timeSinceView, m.lastScrollStats.timeSinceView)
			}
			return m, tea.Batch(statusCmd, cmd)
		}
		return m, statusCmd

	case spinner.TickMsg:
		if m.busy && m.hasPendingToolCalls() && (m.autoScroll || m.viewport.AtBottom()) && m.shouldRefreshNow() {
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
			if m.autoScroll || m.viewport.AtBottom() {
				if m.shouldRefreshNow() {
					m.refreshViewport()
					m.viewportDirty = false
					m.lastRefreshAt = time.Now()
					m.pendingEventRefresh = 0
				} else {
					m.viewportDirty = true
					m.pendingEventRefresh += len(msg.events)
				}
			} else {
				m.viewportDirty = true
				m.pendingEventRefresh += len(msg.events)
			}
		}

		if msg.done {
			m.busy = false
			m.stopwatch, _ = m.stopwatch.Update(stopwatch.StartStopMsg{ID: m.stopwatch.ID()})
			m.showRunResult = true
			if msg.doneErr != nil {
				m.addItem(&session.ErrorItem{Message: msg.doneErr.Error()})
			}
			if turn := m.session.CurrentTurn(); turn != nil {
				turn.End()
				m.persistTurnEnd(turn)
			}
			m.stream = nil
			m.refreshViewport()
			m.lastRefreshAt = time.Now()
			m.viewportDirty = false
			m.pendingEventRefresh = 0
			return m, statusCmd
		}

		return m, tea.Batch(statusCmd, waitForStreamCmd(m.stream))

	case agentEventMsg:
		m.handleAgentEvent(msg.event)
		if m.autoScroll || m.viewport.AtBottom() {
			if m.shouldRefreshNow() {
				m.refreshViewport()
				m.viewportDirty = false
				m.lastRefreshAt = time.Now()
				m.pendingEventRefresh = 0
			} else {
				m.viewportDirty = true
				m.pendingEventRefresh++
			}
		} else {
			m.viewportDirty = true
			m.pendingEventRefresh++
		}
		return m, tea.Batch(statusCmd, waitForStreamCmd(m.stream))

	case agentRunDoneMsg:
		m.busy = false
		m.stopwatch, _ = m.stopwatch.Update(stopwatch.StartStopMsg{ID: m.stopwatch.ID()})
		m.showRunResult = true
		if msg.err != nil {
			m.addItem(&session.ErrorItem{Message: msg.err.Error()})
		}
		if turn := m.session.CurrentTurn(); turn != nil {
			turn.End()
			m.persistTurnEnd(turn)
		}
		m.stream = nil
		m.refreshViewport()
		m.lastRefreshAt = time.Now()
		m.viewportDirty = false
		m.pendingEventRefresh = 0
		return m, statusCmd
	}

	var cmd tea.Cmd
	m.input, cmd = m.input.Update(msg)
	return m, tea.Batch(statusCmd, cmd)
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

	viewportViewStart := time.Now()
	viewportView := m.viewport.View()
	if m.debug {
		m.lastViewStats.viewportViewDuration = time.Since(viewportViewStart)
		m.maxViewStats.viewportViewDuration = maxDuration(m.maxViewStats.viewportViewDuration, m.lastViewStats.viewportViewDuration)
	}

	messagePane := lipgloss.NewStyle().
		Width(mainW).
		Height(messageH).
		Render(viewportView)

	statusStart := time.Now()
	status := components.RenderStatusLine(components.StatusLineParams{
		Busy:          m.busy,
		ShowRunResult: m.showRunResult,
		SpinnerView:   m.spinner.View(),
		Elapsed:       m.stopwatch.Elapsed(),
		InfoColor:     m.theme.Info(),
		MutedColor:    m.theme.Muted(),
		SuccessColor:  m.theme.Success(),
	})
	if m.debug {
		m.lastViewStats.statusDuration = time.Since(statusStart)
		m.maxViewStats.statusDuration = maxDuration(m.maxViewStats.statusDuration, m.lastViewStats.statusDuration)
	}

	inputStart := time.Now()
	inputBox := lipgloss.NewStyle().
		Width(max(1, mainW-2)).
		Height(inputInnerLines).
		Padding(0, 1).
		Border(lipgloss.NormalBorder()).
		BorderForeground(m.theme.Muted()).
		Render(m.input.View())
	if m.debug {
		m.lastViewStats.inputDuration = time.Since(inputStart)
		m.maxViewStats.inputDuration = maxDuration(m.maxViewStats.inputDuration, m.lastViewStats.inputDuration)
	}

	inputBlock := lipgloss.JoinVertical(
		lipgloss.Left,
		status,
		inputBox,
	)
	inputPane := lipgloss.NewStyle().
		Width(mainW).
		Height(inputH).
		Render(inputBlock)

	layoutStart := time.Now()
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
			fmt.Sprintf("Turns: %d", len(m.session.Turns)),
			fmt.Sprintf("Items: %d", m.session.ItemCount()),
		}
		if m.debug {
			cacheHitRate := "n/a"
			if m.lastFormatStats.markdownCalls > 0 {
				rate := (float64(m.lastFormatStats.markdownCacheHits) / float64(m.lastFormatStats.markdownCalls)) * 100
				cacheHitRate = fmt.Sprintf("%.0f%%", rate)
			}
			maxCacheHitRate := "n/a"
			if m.maxFormatStats.markdownCalls > 0 {
				rate := (float64(m.maxFormatStats.markdownCacheHits) / float64(m.maxFormatStats.markdownCalls)) * 100
				maxCacheHitRate = fmt.Sprintf("%.0f%%", rate)
			}
			suppressRemaining := time.Until(m.suppressRefreshUntil)
			if suppressRemaining < 0 {
				suppressRemaining = 0
			}
			sidebarLines = append(sidebarLines,
				"",
				"Debug",
				fmt.Sprintf("Render: %s (max %s)", formatDuration(m.lastRenderLatency), formatDuration(m.maxRenderLatency)),
				fmt.Sprintf("Update gap/dur: %s / %s", formatDuration(m.lastLoopStats.updateGap), formatDuration(m.lastLoopStats.updateDuration)),
				fmt.Sprintf("Update gap/dur max: %s / %s", formatDuration(m.maxLoopStats.updateGap), formatDuration(m.maxLoopStats.updateDuration)),
				fmt.Sprintf("Since View: %s (max %s)", formatDuration(m.lastLoopStats.timeSinceView), formatDuration(m.maxLoopStats.timeSinceView)),
				fmt.Sprintf("Viewport.View: %s (max %s)", formatDuration(m.lastViewStats.viewportViewDuration), formatDuration(m.maxViewStats.viewportViewDuration)),
				fmt.Sprintf("Status/Input: %s / %s", formatDuration(m.lastViewStats.statusDuration), formatDuration(m.lastViewStats.inputDuration)),
				fmt.Sprintf("Status/Input max: %s / %s", formatDuration(m.maxViewStats.statusDuration), formatDuration(m.maxViewStats.inputDuration)),
				fmt.Sprintf("Layout: %s (max %s)", formatDuration(m.lastViewStats.layoutDuration), formatDuration(m.maxViewStats.layoutDuration)),
				fmt.Sprintf("Scroll[%s]: %s (max %s, dy=%d)", m.lastScrollStats.inputType, formatDuration(m.lastScrollStats.viewportUpdate), formatDuration(m.maxScrollStats.viewportUpdate), m.lastScrollStats.deltaRows),
				fmt.Sprintf("Scroll gap/view: %s / %s", formatDuration(m.lastScrollStats.updateGap), formatDuration(m.lastScrollStats.timeSinceView)),
				fmt.Sprintf("Scroll gap/view max: %s / %s", formatDuration(m.maxScrollStats.updateGap), formatDuration(m.maxScrollStats.timeSinceView)),
				fmt.Sprintf("Scroll->View: %s / %s", formatDuration(m.lastScrollStats.inputToViewStart), formatDuration(m.lastScrollStats.inputToViewDone)),
				fmt.Sprintf("Scroll->View max: %s / %s (events max %d)", formatDuration(m.maxScrollStats.inputToViewStart), formatDuration(m.maxScrollStats.inputToViewDone), m.maxScrollStats.coalescedEvents),
				fmt.Sprintf("Frame bytes: %d (max %d)", m.lastFrameBytes, m.maxFrameBytes),
				fmt.Sprintf("Pending refresh events: %d", m.pendingEventRefresh),
				fmt.Sprintf("Refresh suppress: %s", formatDuration(suppressRemaining)),
				fmt.Sprintf("Refresh: %s (max %s)", formatDuration(m.lastFormatStats.refreshDuration), formatDuration(m.maxFormatStats.refreshDuration)),
				fmt.Sprintf("Format: %s (max %s)", formatDuration(m.lastFormatStats.formatDuration), formatDuration(m.maxFormatStats.formatDuration)),
				fmt.Sprintf("SetContent: %s (max %s)", formatDuration(m.lastFormatStats.setContentDuration), formatDuration(m.maxFormatStats.setContentDuration)),
				fmt.Sprintf("Markdown: %s (max %s)", formatDuration(m.lastFormatStats.markdownDuration), formatDuration(m.maxFormatStats.markdownDuration)),
				fmt.Sprintf("MD cache: %s (%d/%d), max %s", cacheHitRate, m.lastFormatStats.markdownCacheHits, m.lastFormatStats.markdownCalls, maxCacheHitRate),
				fmt.Sprintf("MD fallback: %d (max %d)", m.lastFormatStats.markdownFallbacks, m.maxFormatStats.markdownFallbacks),
				fmt.Sprintf("Wrap: %s (max %s)", formatDuration(m.lastFormatStats.wrapDuration), formatDuration(m.maxFormatStats.wrapDuration)),
				fmt.Sprintf("Lines: %d (%d md), max %d (%d md)", m.lastFormatStats.lineCount, m.lastFormatStats.assistantLineCount, m.maxFormatStats.lineCount, m.maxFormatStats.assistantLineCount),
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
	if m.debug {
		m.lastFrameBytes = len(content)
		m.maxFrameBytes = maxInt(m.maxFrameBytes, m.lastFrameBytes)
	}
	if m.debug {
		m.lastViewStats.layoutDuration = time.Since(layoutStart)
		m.maxViewStats.layoutDuration = maxDuration(m.maxViewStats.layoutDuration, m.lastViewStats.layoutDuration)
	}

	v := tea.NewView(content)
	v.AltScreen = true
	v.MouseMode = tea.MouseModeCellMotion
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

func (m *model) handleAgentEvent(e agent.Event) {
	switch e.Type {
	case agent.EventTypeThinkingDelta:
		if data, ok := e.Data.(agent.EventDataThinkingDelta); ok {
			m.appendThinkingDelta(data.Delta)
		}
	case agent.EventTypeMessageDelta:
		data, ok := e.Data.(agent.EventDataMessageDelta)
		if !ok {
			return
		}
		m.appendMessageDelta(data.Delta)

	case agent.EventTypeToolCallStart:
		if data, ok := e.Data.(agent.EventDataToolCallStart); ok {
			m.addToolCall(data.Call)
		}
	case agent.EventTypeToolCallEnd:
		if data, ok := e.Data.(agent.EventDataToolCallEnd); ok {
			m.completeToolCall(data.Call, data.Result)
		}
	case agent.EventTypeError:
		switch data := e.Data.(type) {
		case error:
			m.addItem(&session.ErrorItem{Message: data.Error()})
		case agent.EventDataError:
			if data.Err != nil {
				m.addItem(&session.ErrorItem{Message: data.Err.Error()})
			}
		default:
			m.addItem(&session.ErrorItem{Message: "unknown error"})
		}
	}
}

func (m *model) appendThinkingDelta(delta string) {
	if delta == "" {
		return
	}
	last := m.session.LastItem()
	if thinking, ok := last.(*session.ThinkingBlock); ok {
		thinking.Append(delta)
		m.persistMeta()
		return
	}
	m.addItem(&session.ThinkingBlock{Content: delta})
}

func (m *model) appendMessageDelta(delta string) {
	if delta == "" {
		return
	}
	last := m.session.LastItem()
	// Skip thinking blocks to find the last message
	if _, ok := last.(*session.ThinkingBlock); ok {
		// Check if there's a message before the thinking block in current turn
		items := m.session.CurrentTurnItems()
		for i := len(items) - 2; i >= 0; i-- {
			if msg, ok := items[i].(*session.AssistantMessage); ok {
				msg.Append(delta)
				m.persistMeta()
				return
			}
			break
		}
	}
	if msg, ok := last.(*session.AssistantMessage); ok {
		msg.Append(delta)
		m.persistMeta()
		return
	}
	m.addItem(&session.AssistantMessage{Content: delta})
}

func (m *model) addToolCall(call agent.ToolCall) {
	key := toolCallKey(call)
	item := &session.ToolCallItem{
		ID:        call.ID,
		Name:      call.Name,
		Arguments: call.Arguments,
		Status:    session.ToolCallStatusPending,
	}
	m.toolCalls[key] = item
	m.addItem(item)
}

func (m *model) completeToolCall(call agent.ToolCall, result agent.ToolResult) {
	key := toolCallKey(call)
	if item, ok := m.toolCalls[key]; ok {
		item.Complete(result)
		m.persistItem(m.turnNumber(m.session.CurrentTurn()), item)
		m.persistMeta()
		delete(m.toolCalls, key)
		return
	}
	// Tool call not found, add a completed one
	item := &session.ToolCallItem{
		ID:        call.ID,
		Name:      call.Name,
		Arguments: call.Arguments,
	}
	item.Complete(result)
	m.addItem(item)
}

func (m *model) hasPendingToolCalls() bool {
	return len(m.toolCalls) > 0
}

func (m *model) shouldRefreshNow() bool {
	if !m.suppressRefreshUntil.IsZero() && time.Now().Before(m.suppressRefreshUntil) {
		return false
	}
	if m.lastRefreshAt.IsZero() {
		return true
	}
	return time.Since(m.lastRefreshAt) >= renderRefreshInterval
}

func (m *model) addItem(item session.Item) {
	m.session.AddItem(item)
	m.persistState()
}

func (m *model) addItemToTurn(turn *session.Turn, item session.Item) {
	if turn == nil {
		return
	}
	turn.AddItem(item)
	m.persistState()
}

func (m *model) persistTurnStart(_ *session.Turn) {
	m.persistState()
}

func (m *model) persistTurnEnd(_ *session.Turn) {
	m.persistState()
}

func (m *model) persistMeta() {
	m.persistState()
}

func (m *model) persistItem(_ int, _ session.Item) {
	m.persistState()
}

func (m *model) turnNumber(turn *session.Turn) int {
	for i, t := range m.session.Turns {
		if t == turn {
			return i + 1
		}
	}
	return 0
}

func (m *model) persistState() {
	if m.storage == nil {
		return
	}
	if err := m.storage.Save(m.session); err != nil {
		fmt.Fprintf(os.Stderr, "failed to persist session: %v\n", err)
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

	m.viewport.SetWidth(mainW)
	m.viewport.SetHeight(messageH)
	m.input.SetWidth(max(1, mainW-4))
	m.input.SetHeight(inputInnerLines)
}

func (m *model) refreshViewport() {
	refreshStart := time.Now()
	prevOffset := m.viewport.YOffset()
	wasAtBottom := m.viewport.AtBottom()

	formatStart := time.Now()
	content, stats := m.formatSessionForViewport(m.viewport.Width(), m.debug)
	if m.debug {
		stats.formatDuration = time.Since(formatStart)
	}

	setContentStart := time.Now()
	if content != m.lastViewportContent {
		m.viewport.SetContent(content)
		m.lastViewportContent = content
	}
	if m.debug {
		stats.setContentDuration = time.Since(setContentStart)
		stats.refreshDuration = time.Since(refreshStart)
		m.lastFormatStats = stats
		m.maxFormatStats.refreshDuration = maxDuration(m.maxFormatStats.refreshDuration, stats.refreshDuration)
		m.maxFormatStats.formatDuration = maxDuration(m.maxFormatStats.formatDuration, stats.formatDuration)
		m.maxFormatStats.setContentDuration = maxDuration(m.maxFormatStats.setContentDuration, stats.setContentDuration)
		m.maxFormatStats.markdownDuration = maxDuration(m.maxFormatStats.markdownDuration, stats.markdownDuration)
		m.maxFormatStats.wrapDuration = maxDuration(m.maxFormatStats.wrapDuration, stats.wrapDuration)
		m.maxFormatStats.lineCount = maxInt(m.maxFormatStats.lineCount, stats.lineCount)
		m.maxFormatStats.assistantLineCount = maxInt(m.maxFormatStats.assistantLineCount, stats.assistantLineCount)
		m.maxFormatStats.markdownCalls = maxInt(m.maxFormatStats.markdownCalls, stats.markdownCalls)
		m.maxFormatStats.markdownCacheHits = maxInt(m.maxFormatStats.markdownCacheHits, stats.markdownCacheHits)
		m.maxFormatStats.markdownFallbacks = maxInt(m.maxFormatStats.markdownFallbacks, stats.markdownFallbacks)
	}

	if m.autoScroll || wasAtBottom {
		m.viewport.GotoBottom()
		m.autoScroll = true
		m.pendingEventRefresh = 0
		return
	}
	m.viewport.SetYOffset(prevOffset)
	m.pendingEventRefresh = 0
}

func (m *model) formatSessionForViewport(width int, collectPerf bool) (string, formatPerfStats) {
	stats := formatPerfStats{}
	items := m.session.AllItems()
	if len(items) == 0 {
		return "hh-cli ready", stats
	}
	if width <= 0 {
		return m.formatSessionRaw(), stats
	}

	if collectPerf {
		stats.lineCount = len(items)
	}

	renderer := m.getMarkdownRenderer(width)
	wrapped := make([]string, 0, len(items))

	for _, item := range items {
		switch v := item.(type) {
		case *session.UserMessage:
			wrapStart := time.Now()
			wrapped = append(wrapped, components.WrapLine("user: "+v.Content, width)...)
			if collectPerf {
				stats.wrapDuration += time.Since(wrapStart)
			}

		case *session.AssistantMessage:
			if collectPerf {
				stats.assistantLineCount++
			}
			wrapped = append(wrapped, "assistant:")
			renderedMarkdown, markdownStats := m.renderMarkdown(v.Content, width, renderer)
			if collectPerf {
				stats.markdownDuration += markdownStats.renderDuration
				stats.markdownCalls += markdownStats.calls
				stats.markdownCacheHits += markdownStats.cacheHits
				if markdownStats.fallbackToWrap {
					stats.markdownFallbacks++
				}
			}
			wrapped = append(wrapped, renderedMarkdown)

		case *session.ThinkingBlock:
			wrapStart := time.Now()
			wrapped = append(wrapped, components.WrapLine("thinking: "+v.Content, width)...)
			if collectPerf {
				stats.wrapDuration += time.Since(wrapStart)
			}

		case *session.ToolCallItem:
			m.formatToolCallItem(v, width, &wrapped, &stats, collectPerf)

		case *session.ErrorItem:
			wrapStart := time.Now()
			wrapped = append(wrapped, components.WrapLine("error: "+v.Message, width)...)
			if collectPerf {
				stats.wrapDuration += time.Since(wrapStart)
			}
		}
	}

	return strings.Join(wrapped, "\n"), stats
}

func (m *model) formatSessionRaw() string {
	var lines []string
	for _, item := range m.session.AllItems() {
		switch v := item.(type) {
		case *session.UserMessage:
			lines = append(lines, "user: "+v.Content)
		case *session.AssistantMessage:
			lines = append(lines, "assistant: "+v.Content)
		case *session.ThinkingBlock:
			lines = append(lines, "thinking: "+v.Content)
		case *session.ToolCallItem:
			lines = append(lines, formatToolCallBodyRaw(v))
		case *session.ErrorItem:
			lines = append(lines, "error: "+v.Message)
		}
	}
	return strings.Join(lines, "\n")
}

func (m *model) formatToolCallItem(item *session.ToolCallItem, width int, wrapped *[]string, stats *formatPerfStats, collectPerf bool) {
	body := formatToolCallBodyWithResult(item)

	switch item.Status {
	case session.ToolCallStatusPending:
		line := components.RenderPendingToolCallLine(body, m.spinner.View())
		wrapStart := time.Now()
		*wrapped = append(*wrapped, components.WrapLine(line, width)...)
		if collectPerf {
			stats.wrapDuration += time.Since(wrapStart)
		}

	case session.ToolCallStatusSuccess:
		*wrapped = append(*wrapped, components.RenderCompletedToolCallLine(body, true, width, m.theme.Success(), m.theme.Error())...)

	case session.ToolCallStatusError:
		*wrapped = append(*wrapped, components.RenderCompletedToolCallLine(body, false, width, m.theme.Success(), m.theme.Error())...)
	}
}

func formatToolCallBodyWithResult(item *session.ToolCallItem) string {
	args := strings.TrimSpace(item.Arguments)
	if args == "" || args == "{}" {
		args = ""
	}

	const maxArgLen = 80
	if args != "" {
		runes := []rune(args)
		if len(runes) > maxArgLen {
			args = string(runes[:maxArgLen-1]) + "…"
		}
	}

	body := item.Name
	if args != "" {
		body = fmt.Sprintf("%s %s", item.Name, args)
	}

	// Add result summary for completed tool calls
	if summary := item.ResultSummary(); summary != "" {
		body = fmt.Sprintf("%s [%s]", body, summary)
	}

	return body
}

func formatToolCallBody(name, args string) string {
	args = strings.TrimSpace(args)
	if args == "" || args == "{}" {
		return name
	}

	const maxArgLen = 120
	runes := []rune(args)
	if len(runes) > maxArgLen {
		args = string(runes[:maxArgLen-1]) + "…"
	}

	return fmt.Sprintf("%s %s", name, args)
}

func formatToolCallBodyRaw(item *session.ToolCallItem) string {
	status := "pending"
	switch item.Status {
	case session.ToolCallStatusSuccess:
		status = "success"
	case session.ToolCallStatusError:
		status = "error"
	}
	body := formatToolCallBody(item.Name, item.Arguments)
	return fmt.Sprintf("tool_call_%s: %s", status, body)
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

func (m *model) renderMarkdown(content string, width int, renderer *glamour.TermRenderer) (string, markdownPerfStats) {
	stats := markdownPerfStats{}
	if strings.TrimSpace(content) == "" {
		return "", stats
	}
	stats.calls = 1

	cacheKey := fmt.Sprintf("%d:%s", width, content)
	if cached, ok := m.markdownCache[cacheKey]; ok {
		stats.cacheHits = 1
		return cached, stats
	}

	if renderer == nil {
		fallback := strings.Join(components.WrapLine(content, width), "\n")
		m.markdownCache[cacheKey] = fallback
		return fallback, stats
	}

	renderStart := time.Now()
	rendered, err := renderer.Render(content)
	stats.renderDuration = time.Since(renderStart)
	if err != nil {
		fallback := strings.Join(components.WrapLine(content, width), "\n")
		m.markdownCache[cacheKey] = fallback
		return fallback, stats
	}

	trimmed := strings.TrimRight(rendered, "\n")
	if len(trimmed) > markdownRenderByteBudget {
		fallback := strings.Join(components.WrapLine(content, width), "\n")
		m.markdownCache[cacheKey] = fallback
		stats.fallbackToWrap = true
		return fallback, stats
	}
	m.markdownCache[cacheKey] = trimmed
	return trimmed, stats
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

		switch first := msg.(type) {
		case agentEventMsg:
			events := []agent.Event{first.event}
			for i := 1; i < streamBatchMaxEvents; i++ {
				select {
				case next, ok := <-ch:
					if !ok {
						return streamBatchMsg{events: events}
					}
					switch v := next.(type) {
					case agentEventMsg:
						events = append(events, v.event)
					case agentRunDoneMsg:
						return streamBatchMsg{events: events, done: true, doneErr: v.err}
					default:
						return streamBatchMsg{events: events}
					}
				default:
					return streamBatchMsg{events: events}
				}
			}
			return streamBatchMsg{events: events}

		case agentRunDoneMsg:
			return streamBatchMsg{done: true, doneErr: first.err}

		default:
			return msg
		}
	}
}

func ternary[T any](cond bool, ifTrue, ifFalse T) T {
	if cond {
		return ifTrue
	}
	return ifFalse
}

func maxDuration(a, b time.Duration) time.Duration {
	if a > b {
		return a
	}
	return b
}

func maxInt(a, b int) int {
	if a > b {
		return a
	}
	return b
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

const renderRefreshInterval = 33 * time.Millisecond
const scrollPriorityWindow = 120 * time.Millisecond
const streamBatchMaxEvents = 64
const markdownRenderByteBudget = 16000

func toolCallKey(call agent.ToolCall) string {
	if call.ID != "" {
		return call.ID
	}
	return call.Name + "|" + call.Arguments
}
