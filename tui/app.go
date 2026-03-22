package tui

import (
	"context"
	"fmt"
	"os"
	"reflect"
	"strconv"
	"strings"
	"time"

	"charm.land/bubbles/v2/spinner"
	"charm.land/bubbles/v2/stopwatch"
	"charm.land/bubbles/v2/textarea"
	tea "charm.land/bubbletea/v2"
	"github.com/charmbracelet/glamour"
	"github.com/charmbracelet/lipgloss"
	"github.com/charmbracelet/x/ansi"
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

type formatPerfStats struct{}

type markdownPerfStats struct {
	fallbackToWrap bool
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
			m.autoScroll = m.isListAtBottom(m.messageWidth, m.messageHeight)
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
					viewportUpdate: time.Since(scrollUpdateStart),
					deltaRows:      deltaRows,
					updateGap:      updateGap,
					timeSinceView:  timeSinceView,
				}
				m.maxScrollStats.viewportUpdate = maxDuration(m.maxScrollStats.viewportUpdate, m.lastScrollStats.viewportUpdate)
				m.maxScrollStats.updateGap = maxDuration(m.maxScrollStats.updateGap, m.lastScrollStats.updateGap)
				m.maxScrollStats.timeSinceView = maxDuration(m.maxScrollStats.timeSinceView, m.lastScrollStats.timeSinceView)
			}
			if deltaRows == 0 {
				deltaRows = m.currentListOffset(m.messageWidth) - prevOffset
			}
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
			m.autoScroll = m.isListAtBottom(m.messageWidth, m.messageHeight)
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
					viewportUpdate: time.Since(scrollUpdateStart),
					deltaRows:      deltaRows,
					updateGap:      updateGap,
					timeSinceView:  timeSinceView,
				}
				m.maxScrollStats.viewportUpdate = maxDuration(m.maxScrollStats.viewportUpdate, m.lastScrollStats.viewportUpdate)
				m.maxScrollStats.updateGap = maxDuration(m.maxScrollStats.updateGap, m.lastScrollStats.updateGap)
				m.maxScrollStats.timeSinceView = maxDuration(m.maxScrollStats.timeSinceView, m.lastScrollStats.timeSinceView)
			}
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
			if m.autoScroll || m.isListAtBottom(m.messageWidth, m.messageHeight) {
				if m.shouldRefreshNow() {
					m.refreshViewport()
					m.viewportDirty = false
					m.lastRefreshAt = time.Now()
				} else {
					m.viewportDirty = true
				}
			} else {
				m.viewportDirty = true
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
			return m, statusCmd
		}

		return m, tea.Batch(statusCmd, waitForStreamCmd(m.stream))

	case agentEventMsg:
		m.handleAgentEvent(msg.event)
		if m.autoScroll || m.isListAtBottom(m.messageWidth, m.messageHeight) {
			if m.shouldRefreshNow() {
				m.refreshViewport()
				m.viewportDirty = false
				m.lastRefreshAt = time.Now()
			} else {
				m.viewportDirty = true
			}
		} else {
			m.viewportDirty = true
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

	viewportView := m.renderMessageList(mainW, messageH)

	messagePane := lipgloss.NewStyle().
		Width(mainW).
		Height(messageH).
		Render(viewportView)

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
			fmt.Sprintf("Turns: %d", len(m.session.Turns)),
			fmt.Sprintf("Items: %d", m.session.ItemCount()),
		}
		if m.debug {
			sidebarLines = append(sidebarLines,
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
	wasAtBottom := m.isListAtBottom(m.messageWidth, m.messageHeight)
	m.messageWidth = mainW
	m.messageHeight = messageH
	if m.autoScroll || wasAtBottom {
		m.scrollListToBottom(m.messageWidth, m.messageHeight)
	} else {
		m.clampListOffset(m.messageWidth, m.messageHeight)
	}
	m.input.SetWidth(max(1, mainW-4))
	m.input.SetHeight(inputInnerLines)
}

func (m *model) refreshViewport() {
	if m.messageWidth <= 0 || m.messageHeight <= 0 {
		return
	}
	if m.autoScroll || m.isListAtBottom(m.messageWidth, m.messageHeight) {
		m.scrollListToBottom(m.messageWidth, m.messageHeight)
		m.autoScroll = true
		return
	}
	m.clampListOffset(m.messageWidth, m.messageHeight)
}

func (m *model) renderMessageList(width, height int) string {
	if width <= 0 || height <= 0 {
		return ""
	}
	items := m.session.AllItems()
	if len(items) == 0 {
		return ""
	}

	m.clampListOffset(width, height)
	renderer := m.getMarkdownRenderer(width)

	visible := make([]string, 0, height)
	idx := m.listOffsetIdx
	offset := m.listOffsetLine

	for len(visible) < height && idx < len(items) {
		lines := m.renderItemLines(items[idx], width, renderer)
		if len(lines) == 0 {
			lines = []string{""}
		}
		if offset < len(lines) {
			visible = append(visible, lines[offset:]...)
		}
		idx++
		offset = 0
	}

	if len(visible) > height {
		visible = visible[:height]
	}

	for len(visible) < height {
		visible = append(visible, "")
	}

	return strings.Join(visible, "\n")
}

func (m *model) handleScrollKey(msg tea.KeyPressMsg) (bool, int) {
	if m.messageWidth <= 0 || m.messageHeight <= 0 {
		return false, 0
	}

	lines := 0
	moveToTop := false
	moveToBottom := false

	switch msg.String() {
	case "up":
		lines = -1
	case "down":
		lines = 1
	case "pgup":
		lines = -max(1, m.messageHeight-1)
	case "pgdown":
		lines = max(1, m.messageHeight-1)
	case "ctrl+u":
		lines = -max(1, m.messageHeight/2)
	case "ctrl+d":
		lines = max(1, m.messageHeight/2)
	case "home":
		moveToTop = true
	case "end":
		moveToBottom = true
	default:
		return false, 0
	}

	before := m.currentListOffset(m.messageWidth)
	if moveToTop {
		m.listOffsetIdx = 0
		m.listOffsetLine = 0
	} else if moveToBottom {
		m.scrollListToBottom(m.messageWidth, m.messageHeight)
	} else {
		m.scrollListBy(lines, m.messageWidth, m.messageHeight)
	}
	after := m.currentListOffset(m.messageWidth)

	if before == after {
		return false, 0
	}
	return true, after - before
}

func (m *model) handleMouseWheelScroll(msg tea.MouseWheelMsg) int {
	if m.messageWidth <= 0 || m.messageHeight <= 0 {
		return 0
	}

	const wheelStep = 3
	before := m.currentListOffset(m.messageWidth)

	switch msg.Mouse().Button {
	case tea.MouseWheelUp:
		m.scrollListBy(-wheelStep, m.messageWidth, m.messageHeight)
	case tea.MouseWheelDown:
		m.scrollListBy(wheelStep, m.messageWidth, m.messageHeight)
	default:
		s := msg.String()
		if strings.Contains(s, "wheelup") {
			m.scrollListBy(-wheelStep, m.messageWidth, m.messageHeight)
		} else if strings.Contains(s, "wheeldown") {
			m.scrollListBy(wheelStep, m.messageWidth, m.messageHeight)
		}
	}

	after := m.currentListOffset(m.messageWidth)
	return after - before
}

func (m *model) isListAtBottom(width, height int) bool {
	items := m.session.AllItems()
	if len(items) == 0 || width <= 0 || height <= 0 {
		return true
	}
	renderer := m.getMarkdownRenderer(width)
	total := 0
	for i := m.listOffsetIdx; i < len(items); i++ {
		total += len(m.renderItemLines(items[i], width, renderer))
		if total > height+m.listOffsetLine {
			return false
		}
	}
	return total-m.listOffsetLine <= height
}

func (m *model) clampListOffset(width, height int) {
	items := m.session.AllItems()
	if len(items) == 0 {
		m.listOffsetIdx = 0
		m.listOffsetLine = 0
		return
	}
	if m.listOffsetIdx < 0 {
		m.listOffsetIdx = 0
	}
	if m.listOffsetIdx >= len(items) {
		m.listOffsetIdx = len(items) - 1
		m.listOffsetLine = 0
	}
	lastIdx, lastLine := m.lastListOffset(width, height)
	if m.listOffsetIdx > lastIdx || (m.listOffsetIdx == lastIdx && m.listOffsetLine > lastLine) {
		m.listOffsetIdx = lastIdx
		m.listOffsetLine = lastLine
	}
	if m.listOffsetLine < 0 {
		m.listOffsetLine = 0
	}
}

func (m *model) scrollListToBottom(width, height int) {
	idx, line := m.lastListOffset(width, height)
	m.listOffsetIdx = idx
	m.listOffsetLine = line
}

func (m *model) lastListOffset(width, height int) (int, int) {
	items := m.session.AllItems()
	if len(items) == 0 {
		return 0, 0
	}
	renderer := m.getMarkdownRenderer(width)
	total := 0
	idx := len(items) - 1
	for ; idx >= 0; idx-- {
		total += len(m.renderItemLines(items[idx], width, renderer))
		if total > height {
			break
		}
	}
	if idx < 0 {
		idx = 0
	}
	lineOffset := max(total-height, 0)
	return idx, lineOffset
}

func (m *model) scrollListBy(lines, width, height int) {
	if lines == 0 {
		return
	}
	items := m.session.AllItems()
	if len(items) == 0 {
		return
	}
	renderer := m.getMarkdownRenderer(width)
	if lines > 0 {
		m.listOffsetLine += lines
		for m.listOffsetIdx < len(items) {
			itemHeight := len(m.renderItemLines(items[m.listOffsetIdx], width, renderer))
			if itemHeight <= 0 {
				itemHeight = 1
			}
			if m.listOffsetLine < itemHeight {
				break
			}
			m.listOffsetLine -= itemHeight
			m.listOffsetIdx++
			if m.listOffsetIdx >= len(items) {
				m.scrollListToBottom(width, height)
				return
			}
		}
		lastIdx, lastLine := m.lastListOffset(width, height)
		if m.listOffsetIdx > lastIdx || (m.listOffsetIdx == lastIdx && m.listOffsetLine > lastLine) {
			m.listOffsetIdx = lastIdx
			m.listOffsetLine = lastLine
		}
		return
	}

	m.listOffsetLine += lines
	for m.listOffsetLine < 0 {
		m.listOffsetIdx--
		if m.listOffsetIdx < 0 {
			m.listOffsetIdx = 0
			m.listOffsetLine = 0
			return
		}
		itemHeight := len(m.renderItemLines(items[m.listOffsetIdx], width, renderer))
		if itemHeight <= 0 {
			itemHeight = 1
		}
		m.listOffsetLine += itemHeight
	}
}

func (m *model) currentListOffset(width int) int {
	items := m.session.AllItems()
	if len(items) == 0 || width <= 0 {
		return 0
	}
	renderer := m.getMarkdownRenderer(width)
	offset := 0
	maxIdx := min(m.listOffsetIdx, len(items))
	for i := 0; i < maxIdx; i++ {
		offset += len(m.renderItemLines(items[i], width, renderer))
	}
	offset += m.listOffsetLine
	return offset
}

func (m *model) renderItemLines(item session.Item, width int, renderer *glamour.TermRenderer) []string {
	if cached, ok := m.getCachedRenderedItem(item, width); ok {
		return cached
	}

	var lines []string
	needsNormalize := true
	switch v := item.(type) {
	case *session.UserMessage:
		userLines := components.WrapLine(v.Content, max(1, width-3))
		if len(userLines) == 0 {
			userLines = []string{""}
		}
		prefix := lipgloss.
			NewStyle().
			Border(lipgloss.NormalBorder(), false).
			BorderLeft(true).
			PaddingLeft(1).
			BorderLeftForeground(m.theme.Accent())
		lines = make([]string, 0, len(userLines))
		for _, line := range userLines {
			lines = append(lines, prefix.Render(line))
		}

	case *session.AssistantMessage:
		renderedMarkdown, _ := m.renderMarkdown(v.Content, max(1, width-2), renderer)
		assistantLines := strings.Split(renderedMarkdown, "\n")
		lines = make([]string, 0, len(assistantLines))
		for _, line := range assistantLines {
			lines = append(lines, trimOneLeadingSpace(line))
		}

	case *session.ThinkingBlock:
		renderedMarkdown, _ := m.renderMarkdown(v.Content, max(1, width-2-len("Thinking: ")), renderer)
		plainMarkdown := ansi.Strip(renderedMarkdown)
		plainMarkdown = strings.Trim(plainMarkdown, "\r\n")
		thinkingLines := strings.Split(plainMarkdown, "\n")
		for len(thinkingLines) > 0 && strings.TrimSpace(thinkingLines[0]) == "" {
			thinkingLines = thinkingLines[1:]
		}
		if len(thinkingLines) == 0 {
			thinkingLines = []string{""}
		}
		muted := lipgloss.NewStyle().Foreground(m.theme.Muted())
		thinkingPrefix := lipgloss.NewStyle().Foreground(m.theme.Warning()).Render("Thinking: ")
		lines = make([]string, 0, len(thinkingLines))
		for i, line := range thinkingLines {
			line = strings.TrimRight(line, "\r")
			line = strings.TrimLeft(line, " ")
			if i == 0 {
				lines = append(lines, "  "+thinkingPrefix+muted.Render(line))
				continue
			}
			lines = append(lines, "  "+muted.Render(line))
		}
		needsNormalize = false

	case *session.ToolCallItem:
		toolLines := components.RenderToolCall(v, max(1, width-2), m.theme.Success(), m.theme.Error(), m.theme.Info(), m.theme.Success(), m.theme.Error())
		lines = prefixedLines(toolLines, "  ")

	case *session.ErrorItem:
		lines = prefixedLines(components.WrapLine("error: "+v.Message, max(1, width-2)), "  ")

	default:
		lines = []string{""}
	}

	if len(lines) == 0 {
		lines = []string{""}
	}
	if needsNormalize {
		lines = normalizeLinesForWidth(lines, width)
	}
	m.setCachedRenderedItem(item, width, lines)
	return lines
}

func normalizeLinesForWidth(lines []string, width int) []string {
	if width <= 0 || len(lines) == 0 {
		return lines
	}
	out := make([]string, 0, len(lines))
	for _, line := range lines {
		wrapped := ansi.Hardwrap(line, width, true)
		parts := strings.Split(wrapped, "\n")
		if len(parts) == 0 {
			out = append(out, "")
			continue
		}
		out = append(out, parts...)
	}
	return out
}

func prefixedLines(lines []string, prefix string) []string {
	if len(lines) == 0 {
		return []string{prefix}
	}
	out := make([]string, 0, len(lines))
	for _, line := range lines {
		out = append(out, prefix+line)
	}
	return out
}

func trimOneLeadingSpace(line string) string {
	if strings.HasPrefix(line, " ") {
		return line[1:]
	}
	return line
}

func (m *model) formatSessionForViewport(width int, _ bool) (string, formatPerfStats) {
	stats := formatPerfStats{}
	items := m.session.AllItems()
	if len(items) == 0 {
		return "", stats
	}
	if width <= 0 {
		return m.formatSessionRaw(), stats
	}

	renderer := m.getMarkdownRenderer(width)
	wrapped := make([]string, 0, len(items))

	for _, item := range items {
		wrapped = append(wrapped, m.renderItemLines(item, width, renderer)...)
	}

	return strings.Join(wrapped, "\n"), stats
}

func (m *model) getCachedRenderedItem(item session.Item, width int) ([]string, bool) {
	if m.itemRenderCache == nil {
		return nil, false
	}
	if item == nil {
		return nil, false
	}
	if tc, ok := item.(*session.ToolCallItem); ok && tc.Status == session.ToolCallStatusPending {
		return nil, false
	}
	key := itemCacheKey(item)
	if key == 0 {
		return nil, false
	}
	entry, ok := m.itemRenderCache[key]
	if !ok {
		return nil, false
	}
	sig, ok := itemCacheSignature(item)
	if !ok || entry.width != width || entry.signature != sig {
		return nil, false
	}
	return cloneStringSlice(entry.lines), true
}

func (m *model) setCachedRenderedItem(item session.Item, width int, lines []string) {
	if m.itemRenderCache == nil {
		return
	}
	if item == nil {
		return
	}
	if tc, ok := item.(*session.ToolCallItem); ok && tc.Status == session.ToolCallStatusPending {
		return
	}
	key := itemCacheKey(item)
	if key == 0 {
		return
	}
	sig, ok := itemCacheSignature(item)
	if !ok {
		return
	}
	m.itemRenderCache[key] = itemRenderCacheEntry{
		width:     width,
		signature: sig,
		lines:     cloneStringSlice(lines),
	}
}

func itemCacheKey(item session.Item) uintptr {
	v := reflect.ValueOf(item)
	if !v.IsValid() || v.Kind() != reflect.Ptr || v.IsNil() {
		return 0
	}
	return v.Pointer()
}

func itemCacheSignature(item session.Item) (string, bool) {
	switch v := item.(type) {
	case *session.UserMessage:
		return "user:" + v.Content, true
	case *session.AssistantMessage:
		return "assistant:" + v.Content, true
	case *session.ThinkingBlock:
		return "thinking:" + v.Content, true
	case *session.ToolCallItem:
		summary := ""
		if v.Result != nil {
			summary = v.ResultSummary()
		}
		return fmt.Sprintf("tool:%d:%s:%s:%s:%t", v.Status, v.Name, v.Arguments, summary, v.Result != nil), true
	case *session.ErrorItem:
		return "error:" + v.Message, true
	default:
		return "", false
	}
}

func cloneStringSlice(in []string) []string {
	out := make([]string, len(in))
	copy(out, in)
	return out
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

	cacheKey := fmt.Sprintf("%d:%s", width, content)
	if cached, ok := m.markdownCache[cacheKey]; ok {
		return cached, stats
	}

	if renderer == nil {
		fallback := strings.Join(components.WrapLine(content, width), "\n")
		m.markdownCache[cacheKey] = fallback
		return fallback, stats
	}

	rendered, err := renderer.Render(content)
	if err != nil {
		fallback := strings.Join(components.WrapLine(content, width), "\n")
		m.markdownCache[cacheKey] = fallback
		return fallback, stats
	}

	trimmed := strings.TrimRight(rendered, "\n")
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

func min(a, b int) int {
	if a < b {
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

func toolCallKey(call agent.ToolCall) string {
	if call.ID != "" {
		return call.ID
	}
	return call.Name + "|" + call.Arguments
}
