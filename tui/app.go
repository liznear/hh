package tui

import (
	"bytes"
	"charm.land/bubbles/v2/spinner"
	"charm.land/bubbles/v2/stopwatch"
	"charm.land/bubbles/v2/textarea"
	tea "charm.land/bubbletea/v2"
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"github.com/charmbracelet/lipgloss"
	"github.com/liznear/hh/agent"
	"github.com/liznear/hh/config"
	"github.com/liznear/hh/tools"
	"github.com/liznear/hh/tui/commands"
	"github.com/liznear/hh/tui/session"
	"os"
	"os/exec"
	"strconv"
	"strings"
	"syscall"
	"time"
)

type model struct {
	runner    *agent.AgentRunner
	agentName string
	modelName string
	theme     Theme
	storage   *session.Storage

	State

	config config.Config

	spinner         spinner.Model
	stopwatch       stopwatch.Model
	itemRenderCache map[uintptr]itemRenderCacheEntry
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
	modelPicker  *modelPickerState
	resumePicker *resumePickerState

	width  int
	height int

	input          textarea.Model
	messageWidth   int
	messageHeight  int
	listOffsetIdx  int
	listOffsetLine int

	stream       <-chan tea.Msg
	btwStream    <-chan tea.Msg
	btwStreamCmd tea.Cmd

	taskSessionPlaybackCmd tea.Cmd
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

	currentRunKind runKind

	btwBusy bool

	hideNextUserMessage bool

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

	questionDialog  *questionDialogState
	diffDialog      *diffDialogState
	taskSessionView *taskSessionViewState

	lastRenderLatency time.Duration
	maxRenderLatency  time.Duration
	lastScrollStats   scrollPerfStats
	maxScrollStats    scrollPerfStats

	workingDir               string
	gitBranch                string
	modifiedFiles            []modifiedFileStat
	sidebarModifiedFileLines []sidebarModifiedFileLine
	taskLineClickTargets     []taskLineClickTarget
	taskLiveSessions         map[string]map[int]*taskSessionLiveState
	lastGitRefreshAt         time.Time
	contextWindowUsed        int

	questionPromptedAt       time.Time
	questionLastLatency      time.Duration
	questionSubmittedCount   int
	questionValidationErrors int

	queuedSteering []queuedSteeringMessage

	mentionSuggestions    []mentionSuggestion
	mentionSelectionIndex int

	ephemeralItems []ephemeralItem
}

type sidebarModifiedFileLine struct {
	Line int
	Path string
}

type taskLineClickTarget struct {
	ViewLine         int
	ParentToolCallID string
	TaskIndex        int
	SubAgentName     string
	Task             string
	Status           string
	Error            string
	AgentMessages    []agent.Message
}

type taskSessionLiveState struct {
	SubAgentName string
	Task         string
	Session      *session.State
	PendingTools map[string]*session.ToolCallItem
	Running      bool
}

type ephemeralItem struct {
	turnID     string
	afterIndex int
	item       session.Item
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

type runKind string

const (
	runKindNormal  runKind = "normal"
	runKindCompact runKind = "compact"
)

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
		return m.handleWindowSizeMsg(msg, statusCmd)

	case tea.KeyPressMsg:
		return m.handleKeyPressMsg(msg, statusCmd, updateGap, timeSinceView)

	case tea.MouseWheelMsg:
		return m.handleMouseWheelMsg(msg, statusCmd, updateGap, timeSinceView)

	case tea.MouseClickMsg:
		return m.handleMouseClickMsg(msg, statusCmd)

	case shellCommandDoneMsg:
		return m.handleShellCommandDoneMsg(msg, statusCmd)

	case spinner.TickMsg:
		return m.handleSpinnerTickMsg(statusCmd)

	case agentStreamStartedMsg:
		return m.handleAgentStreamStartedMsg(msg, statusCmd)

	case streamBatchMsg:
		return m.handleStreamBatchMsg(msg, statusCmd)

	case agentEventMsg:
		return m.handleAgentEventMsg(msg, statusCmd)

	case agentRunDoneMsg:
		return m.handleAgentRunDoneMsg(msg, statusCmd)

	case btwRunDoneMsg:
		return m.handleBTWRunDoneMsg(msg, statusCmd)

	case btwStreamBatchMsg:
		return m.handleBTWStreamBatchMsg(msg, statusCmd)

	case btwStreamStartedMsg:
		return m.handleBTWStreamStartedMsg(msg, statusCmd)

	case taskSessionPlaybackTickMsg:
		return m.handleTaskSessionPlaybackTickMsg(statusCmd)
	}

	var cmd tea.Cmd
	m.input, cmd = m.input.Update(msg)
	m.updateMentionAutocomplete()
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

func (m *model) handleAgentEvent(e agent.Event) {
	m.maybeClearQueuedSteering(e)

	switch e.Type {
	case agent.EventTypeTurnStart:
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
	case agent.EventTypeMessage:
		data, ok := e.Data.(agent.EventDataMessage)
		if !ok {
			return
		}
		if data.Message.Role == agent.RoleUser {
			if !m.hideNextUserMessage {
				m.addItem(&session.UserMessage{Content: data.Message.Content})
			}
			m.hideNextUserMessage = false
		}

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
			if errors.Is(data, context.Canceled) && m.cancelledRun {
				return
			}
			m.addItem(&session.ErrorItem{Message: data.Error()})
		case agent.EventDataError:
			if data.Err != nil {
				if errors.Is(data.Err, context.Canceled) && m.cancelledRun {
					return
				}
				m.addItem(&session.ErrorItem{Message: data.Err.Error()})
			}
		default:
			m.addItem(&session.ErrorItem{Message: "unknown error"})
		}
	case agent.EventTypeSessionTitle:
		if data, ok := e.Data.(agent.EventDataSessionTitle); ok {
			m.session.SetTitle(data.Title)
			m.persistMeta()
		}
	case agent.EventTypeTokenUsage:
		if data, ok := e.Data.(agent.EventDataTokenUsage); ok {
			if data.Usage.TotalTokens > 0 {
				m.contextWindowUsed = data.Usage.TotalTokens
			}
		}
	case agent.EventTypeInteractionRequested:
		if data, ok := e.Data.(agent.EventDataInteractionRequested); ok {
			if data.Request.Kind == agent.InteractionKindQuestion || data.Request.Kind == agent.InteractionKindApproval {
				m.openQuestionDialog(data.Request)
			}
		}
	case agent.EventTypeInteractionResponded:
		if data, ok := e.Data.(agent.EventDataInteractionResponded); ok {
			if dlg := m.questionDialog; dlg != nil && dlg.request.InteractionID == data.Response.InteractionID {
				m.closeQuestionDialog()
			}
		}
	case agent.EventTypeInteractionDismissed:
		if data, ok := e.Data.(agent.EventDataInteractionDismissed); ok {
			if dlg := m.questionDialog; dlg != nil && dlg.request.InteractionID == data.InteractionID {
				m.closeQuestionDialog()
			}
		}
	case agent.EventTypeInteractionExpired:
		if data, ok := e.Data.(agent.EventDataInteractionExpired); ok {
			if dlg := m.questionDialog; dlg != nil && dlg.request.InteractionID == data.InteractionID {
				m.closeQuestionDialog()
				m.addItem(&session.ErrorItem{Message: "interaction timed out"})
			}
		}
	case agent.EventTypeTaskProgress:
		if data, ok := e.Data.(agent.EventDataTaskProgress); ok {
			m.handleTaskProgressEvent(data)
		}
	}
}

func (m *model) maybeClearQueuedSteering(e agent.Event) {
	if len(m.queuedSteering) == 0 {
		return
	}

	switch e.Type {
	case agent.EventTypeTurnStart, agent.EventTypeTurnEnd, agent.EventTypeAgentEnd:
		m.queuedSteering = nil
		return
	case agent.EventTypeMessage:
		data, ok := e.Data.(agent.EventDataMessage)
		if ok && data.Message.Role == agent.RoleUser {
			m.queuedSteering = nil
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
	if _, ok := last.(*session.ThinkingBlock); ok {
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
		m.applyToolState(call, result)
		m.lastGitRefreshAt = time.Time{}
		m.persistItem(m.turnNumber(m.session.CurrentTurn()), item)
		m.persistMeta()
		delete(m.toolCalls, key)
		return
	}

	item := &session.ToolCallItem{
		ID:        call.ID,
		Name:      call.Name,
		Arguments: call.Arguments,
	}
	item.Complete(result)
	m.applyToolState(call, result)
	m.lastGitRefreshAt = time.Time{}
	m.addItem(item)
}

func (m *model) applyToolState(call agent.ToolCall, result agent.ToolResult) {
	if result.IsErr {
		return
	}
	if call.Name != "todo_write" {
		return
	}
	items, ok := toSessionTodoItems(result.Result)
	if !ok {
		return
	}
	m.session.SetTodoItems(items)
}

func toSessionTodoItems(raw any) ([]session.TodoItem, bool) {
	if raw == nil {
		return nil, true
	}

	decoded, ok := raw.(tools.TodoWriteResult)
	if !ok {
		ptr, ok := raw.(*tools.TodoWriteResult)
		if ok && ptr != nil {
			decoded = *ptr
		} else {
			buf, err := json.Marshal(raw)
			if err != nil {
				return nil, false
			}
			if err := json.Unmarshal(buf, &decoded); err != nil {
				return nil, false
			}
		}
	}

	items := make([]session.TodoItem, 0, len(decoded.TodoItems))
	for _, item := range decoded.TodoItems {
		items = append(items, session.TodoItem{
			Content: item.Content,
			Status:  session.TodoStatus(item.Status),
		})
	}
	return items, true
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
	if !m.hasSubmittedUserPrompt() {
		return
	}
	if err := m.storage.Save(m.session); err != nil {
		fmt.Fprintf(os.Stderr, "failed to persist session: %v\n", err)
	}
}

func (m *model) hasSubmittedUserPrompt() bool {
	if m.session == nil {
		return false
	}
	for _, item := range m.session.AllItems() {
		if _, ok := item.(*session.UserMessage); ok {
			return true
		}
	}
	return false
}

func (m *model) handleKeyPressMsg(msg tea.KeyPressMsg, statusCmd tea.Cmd, updateGap time.Duration, timeSinceView time.Duration) (tea.Model, tea.Cmd) {
	if updated, cmd, handled := m.handleDialogKeyPress(msg, statusCmd); handled {
		return updated, cmd
	}

	key := msg.Key()
	if key.Code == tea.KeyTab {
		if m.applyMentionAutocomplete() {
			return m, statusCmd
		}
		if !m.busy {
			if err := m.switchToNextAgent(); err != nil {
				m.addItem(&session.ErrorItem{Message: err.Error()})
			}
			return m, statusCmd
		}
	}
	if m.handleMentionSelectionKey(msg) {
		return m, statusCmd
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

	if key.Code == tea.KeyEscape && m.busy {
		m.requestCancelRun()
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
		return m.handleEnterKey(msg, statusCmd)
	}

	switch msg.String() {
	case "ctrl+c":
		return m, tea.Quit
	}
	m.escPending = false

	var cmd tea.Cmd
	m.input, cmd = m.input.Update(msg)
	m.updateMentionAutocomplete()
	return m, tea.Batch(statusCmd, cmd)
}

func (m *model) handleEnterKey(_ tea.KeyPressMsg, statusCmd tea.Cmd) (tea.Model, tea.Cmd) {
	inputValue := m.input.Value()
	prompt := strings.TrimSpace(inputValue)
	if prompt == "" {
		return m, statusCmd
	}

	// Handle slash commands that need custom runtime behavior.
	if inv, ok := commands.ParseInvocation(prompt); ok {
		switch inv.Name {
		case "btw":
			if inv.ArgsRaw == "" {
				m.addItem(&session.ErrorItem{Message: "/btw requires a prompt"})
				m.input.SetValue("")
				m.refreshViewport()
				return m, statusCmd
			}
			if err := m.startBTWRun(inv.ArgsRaw); err != nil {
				m.addItem(&session.ErrorItem{Message: err.Error()})
				m.refreshViewport()
				return m, statusCmd
			}
			m.input.SetValue("")
			cmd := m.btwStreamCmd
			m.btwStreamCmd = nil
			return m, tea.Batch(statusCmd, cmd)
		case "compact":
			if inv.ArgsRaw != "" {
				m.addItem(&session.ErrorItem{Message: "/compact does not accept arguments"})
				m.input.SetValue("")
				m.refreshViewport()
				return m, statusCmd
			}
			if m.busy {
				m.addItem(&session.ErrorItem{Message: "cannot run /compact while another run is active"})
				m.input.SetValue("")
				m.refreshViewport()
				return m, statusCmd
			}
			return m.beginAgentRunWithKind(compactPrompt(), runKindCompact)
		}
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
		return m.beginShellRun(command, true)
	}

	if isShellModeInput(inputValue) {
		command := parseShellCommand(inputValue)
		if strings.TrimSpace(command) == "" {
			return m, statusCmd
		}
		return m.beginShellRun(command, false)
	}

	if m.handleSlashCommand(prompt) {
		m.input.SetValue("")
		m.showRunResult = false
		m.escPending = false
		return m, statusCmd
	}

	if subAgentName, taskPrompt, ok := parseSubAgentInvocation(prompt); ok {
		return m.beginMentionSubAgentRun(subAgentName, taskPrompt)
	}

	return m.beginAgentRun(prompt)
}

func compactPrompt() string {
	return strings.TrimSpace(`Please compact the entire conversation context into a concise, actionable summary.

Structure your response with exactly these sections:
1. Goal
2. Overall plan and current progress
3. Remaining work and next step
4. Lessons learned in previous work which should be remembered and applied for following work

Requirements:
- Use only information from the current session context.
- Be specific and concrete.
- Keep it brief but complete enough to continue work without losing context.`)
}

func (m *model) applyCompactedContext(turn *session.Turn) {
	if m.runner == nil || turn == nil {
		return
	}

	summary := latestAssistantMessage(turn.Items)
	if summary == "" {
		return
	}

	messages := []agent.Message{{Role: agent.RoleAssistant, Content: summary}}
	if err := m.runner.Update(agent.WithMessages(messages)); err != nil {
		m.addItem(&session.ErrorItem{Message: fmt.Sprintf("failed to apply compacted context: %v", err)})
	}
}

func latestAssistantMessage(items []session.Item) string {
	for i := len(items) - 1; i >= 0; i-- {
		msg, ok := items[i].(*session.AssistantMessage)
		if !ok {
			continue
		}
		content := strings.TrimSpace(msg.Content)
		if content != "" {
			return content
		}
	}
	return ""
}

func (m *model) handleShellCommandDoneMsg(msg shellCommandDoneMsg, statusCmd tea.Cmd) (tea.Model, tea.Cmd) {
	if turn := m.session.CurrentTurn(); turn != nil {
		m.addItemToTurn(turn, &session.ShellMessage{Command: msg.command, Output: msg.output})
	}
	m.finalizeRun(msg.err)
	return m, statusCmd
}

func (m *model) handleAgentStreamStartedMsg(msg agentStreamStartedMsg, statusCmd tea.Cmd) (tea.Model, tea.Cmd) {
	m.stream = msg.ch
	return m, tea.Batch(statusCmd, waitForStreamCmd(m.stream))
}

func (m *model) handleStreamBatchMsg(msg streamBatchMsg, statusCmd tea.Cmd) (tea.Model, tea.Cmd) {
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
}

func (m *model) handleAgentEventMsg(msg agentEventMsg, statusCmd tea.Cmd) (tea.Model, tea.Cmd) {
	if msg.btw {
		m.handleBTWEvent(msg.event, msg.btwItem)
		m.refreshAfterStreamEvent()
		return m, tea.Batch(statusCmd, waitForBTWStreamCmd(m.btwStream))
	}
	m.handleAgentEvent(msg.event)
	m.refreshAfterStreamEvent()
	return m, tea.Batch(statusCmd, waitForStreamCmd(m.stream))
}

func (m *model) handleBTWEvent(e agent.Event, item session.Item) {
	exchange, ok := item.(*session.BTWExchange)
	if !ok {
		return
	}
	switch e.Type {
	case agent.EventTypeMessageDelta:
		if data, ok := e.Data.(agent.EventDataMessageDelta); ok {
			exchange.AppendAnswer(data.Delta)
			m.refreshViewport()
		}
	case agent.EventTypeError:
		switch data := e.Data.(type) {
		case error:
			m.addItem(&session.ErrorItem{Message: data.Error()})
		case agent.EventDataError:
			if data.Err != nil {
				m.addItem(&session.ErrorItem{Message: data.Err.Error()})
			}
		}
	}
}

func (m *model) handleAgentRunDoneMsg(msg agentRunDoneMsg, statusCmd tea.Cmd) (tea.Model, tea.Cmd) {
	m.finalizeRun(msg.err)
	return m, statusCmd
}

func (m *model) handleBTWStreamStartedMsg(msg btwStreamStartedMsg, statusCmd tea.Cmd) (tea.Model, tea.Cmd) {
	m.btwStream = msg.ch
	m.btwBusy = true
	return m, tea.Batch(statusCmd, waitForBTWStreamCmd(m.btwStream))
}

func (m *model) handleBTWRunDoneMsg(msg btwRunDoneMsg, statusCmd tea.Cmd) (tea.Model, tea.Cmd) {
	m.btwStream = nil
	m.btwBusy = false
	m.refreshViewport()
	return m, statusCmd
}

func (m *model) handleBTWStreamBatchMsg(msg btwStreamBatchMsg, statusCmd tea.Cmd) (tea.Model, tea.Cmd) {
	if len(msg.events) > 0 {
		for _, e := range msg.events {
			// Find the last BTWExchange item to update
			for i := len(m.ephemeralItems) - 1; i >= 0; i-- {
				if exchange, ok := m.ephemeralItems[i].item.(*session.BTWExchange); ok {
					m.handleBTWEvent(e, exchange)
					break
				}
			}
		}
		m.refreshAfterStreamEvent()
	}

	if msg.done {
		m.btwStream = nil
		m.btwBusy = false
		m.refreshViewport()
		return m, statusCmd
	}

	return m, tea.Batch(statusCmd, waitForBTWStreamCmd(m.btwStream))
}

func (m *model) handleTaskSessionPlaybackTickMsg(statusCmd tea.Cmd) (tea.Model, tea.Cmd) {
	return m, statusCmd
}

func waitForBTWStreamCmd(ch <-chan tea.Msg) tea.Cmd {
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
						return btwStreamBatchMsg{events: events}
					}
					switch v := next.(type) {
					case agentEventMsg:
						events = append(events, v.event)
					case btwRunDoneMsg:
						return btwStreamBatchMsg{events: events, done: true, doneErr: v.err}
					default:
						return btwStreamBatchMsg{events: events}
					}
				default:
					return btwStreamBatchMsg{events: events}
				}
			}
			return btwStreamBatchMsg{events: events}

		case btwRunDoneMsg:
			return btwStreamBatchMsg{done: true, doneErr: first.err}

		default:
			return msg
		}
	}
}

func (m *model) beginRun() context.Context {
	m.busy = true
	m.escPending = false
	m.cancelledRun = false
	m.showRunResult = false
	m.currentRunKind = runKindNormal
	runCtx, cancel := context.WithCancel(context.Background())
	m.runCancel = cancel
	m.refreshViewport()
	return runCtx
}

func (m *model) beginAgentRun(prompt string) (tea.Model, tea.Cmd) {
	return m.beginAgentRunWithKind(prompt, runKindNormal)
}

func (m *model) beginAgentRunWithKind(prompt string, kind runKind) (tea.Model, tea.Cmd) {
	turn := m.session.StartTurn()
	m.persistTurnStart(turn)
	if kind == runKindCompact {
		m.addItemToTurn(turn, &session.CompactionMarker{})
		m.hideNextUserMessage = true
	}
	mentionedFiles := m.collectMentionedFileContents(prompt)
	internalState := buildInternalState(m.session.TodoItems, mentionedFiles)
	m.input.SetValue("")
	m.updateMentionAutocomplete()
	runCtx := m.beginRun()
	m.currentRunKind = kind

	return m, tea.Batch(startAgentStreamCmdWithContext(runCtx, m.runner, prompt, internalState), m.stopwatch.Reset(), m.stopwatch.Start(), func() tea.Msg {
		return m.spinner.Tick()
	})
}

func (m *model) beginMentionSubAgentRun(subAgentName, taskPrompt string) (tea.Model, tea.Cmd) {
	turn := m.session.StartTurn()
	m.persistTurnStart(turn)
	mentionedFiles := m.collectMentionedFileContents(effectiveSubAgentPrompt(subAgentName, taskPrompt))
	internalState := buildInternalState(m.session.TodoItems, mentionedFiles)
	m.input.SetValue("")
	m.updateMentionAutocomplete()
	runCtx := m.beginRun()
	toolCallID := fmt.Sprintf("mention_task_%d", time.Now().UnixNano())

	return m, tea.Batch(startMentionSubAgentStreamCmdWithContext(runCtx, m.config, m.modelName, m.workingDir, subAgentName, taskPrompt, internalState, toolCallID), m.stopwatch.Reset(), m.stopwatch.Start(), func() tea.Msg {
		return m.spinner.Tick()
	})
}

func (m *model) beginShellRun(command string, explicitShellMode bool) (tea.Model, tea.Cmd) {
	turn := m.session.StartTurn()
	m.persistTurnStart(turn)
	m.input.SetValue("")
	if explicitShellMode {
		m.setShellMode(false)
	}
	runCtx := m.beginRun()

	return m, tea.Batch(runShellCommandCmdWithContext(runCtx, command), m.stopwatch.Reset(), m.stopwatch.Start(), func() tea.Msg {
		return m.spinner.Tick()
	})
}

func (m *model) requestCancelRun() {
	if m.escPending {
		if m.runCancel != nil {
			m.runCancel()
		}
		m.cancelledRun = true
		m.escPending = false
		return
	}
	m.escPending = true
}

func (m *model) handleDialogKeyPress(msg tea.KeyPressMsg, statusCmd tea.Cmd) (tea.Model, tea.Cmd, bool) {
	if m.taskSessionView != nil {
		if m.handleTaskSessionViewKey(msg) {
			m.refreshViewport()
			return m, statusCmd, true
		}
	}

	if m.diffDialog != nil {
		if m.handleDiffDialogKey(msg) {
			m.refreshViewport()
			return m, statusCmd, true
		}
	}

	if m.questionDialog != nil {
		if m.handleQuestionDialogKey(msg) {
			m.refreshViewport()
			return m, statusCmd, true
		}
	}

	if m.modelPicker != nil {
		if m.handleModelPickerKey(msg) {
			m.showRunResult = false
			m.escPending = false
			return m, statusCmd, true
		}
	}

	if m.resumePicker != nil {
		if m.handleResumePickerKey(msg) {
			m.showRunResult = false
			m.escPending = false
			return m, statusCmd, true
		}
	}

	return m, nil, false
}

func (m *model) switchToNextAgent() error {
	if m == nil {
		return nil
	}
	if m.runner == nil {
		return fmt.Errorf("runner unavailable")
	}

	agentNames, err := listAvailableAgents()
	if err != nil {
		return err
	}
	if len(agentNames) == 0 {
		return fmt.Errorf("no agents available")
	}

	current := strings.TrimSpace(m.agentName)
	nextIdx := 0
	for i, name := range agentNames {
		if name == current {
			nextIdx = (i + 1) % len(agentNames)
			break
		}
	}
	nextAgentName := agentNames[nextIdx]

	if err := updateRunnerForAgent(m.runner, nextAgentName, m.config, m.workingDir); err != nil {
		return fmt.Errorf("switch agent to %q: %w", nextAgentName, err)
	}

	m.agentName = nextAgentName
	m.showRunResult = false
	m.escPending = false
	m.refreshViewport()
	return nil
}

func (m *model) handleMouseWheelMsg(msg tea.MouseWheelMsg, statusCmd tea.Cmd, updateGap time.Duration, timeSinceView time.Duration) (tea.Model, tea.Cmd) {
	if m.handleTaskSessionViewWheel(msg) {
		m.refreshViewport()
		return m, statusCmd
	}
	if m.handleDiffDialogWheel(msg) {
		m.refreshViewport()
		return m, statusCmd
	}

	scrollUpdateStart := time.Now()
	deltaRows := m.handleMouseWheelScroll(msg)
	if deltaRows != 0 {
		m.recordScrollInteraction("mouse", scrollUpdateStart, deltaRows, updateGap, timeSinceView)
		return m, statusCmd
	}
	return m, statusCmd
}

func (m *model) handleMouseClickMsg(msg tea.MouseClickMsg, statusCmd tea.Cmd) (tea.Model, tea.Cmd) {
	if m.handleTaskLineClick(msg.Mouse().X, msg.Mouse().Y) {
		m.refreshViewport()
		cmd := m.taskSessionPlaybackCmd
		m.taskSessionPlaybackCmd = nil
		return m, tea.Batch(statusCmd, cmd)
	}
	if m.handleSidebarModifiedFileClick(msg.Mouse().X, msg.Mouse().Y) {
		m.refreshViewport()
		return m, statusCmd
	}
	return m, statusCmd
}

func (m *model) handleSpinnerTickMsg(statusCmd tea.Cmd) (tea.Model, tea.Cmd) {
	if m.busy && m.hasPendingToolCalls() && (m.autoScroll || m.isListAtBottom(m.messageWidth, m.messageHeight)) && m.shouldRefreshNow() {
		m.refreshViewport()
		m.lastRefreshAt = time.Now()
	} else if m.busy && m.hasPendingToolCalls() {
		m.viewportDirty = true
	}
	return m, statusCmd
}

func (m *model) handleWindowSizeMsg(msg tea.WindowSizeMsg, statusCmd tea.Cmd) (tea.Model, tea.Cmd) {
	m.width = msg.Width
	m.height = msg.Height
	m.syncLayout()
	m.refreshViewport()
	return m, statusCmd
}

func (m *model) recordScrollInteraction(inputType string, startedAt time.Time, deltaRows int, updateGap time.Duration, timeSinceView time.Duration) {
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
	if !m.debug {
		return
	}
	m.lastScrollStats = scrollPerfStats{
		inputType:      inputType,
		viewportUpdate: time.Since(startedAt),
		deltaRows:      deltaRows,
		updateGap:      updateGap,
		timeSinceView:  timeSinceView,
	}
	m.maxScrollStats.viewportUpdate = maxDuration(m.maxScrollStats.viewportUpdate, m.lastScrollStats.viewportUpdate)
	m.maxScrollStats.updateGap = maxDuration(m.maxScrollStats.updateGap, m.lastScrollStats.updateGap)
	m.maxScrollStats.timeSinceView = maxDuration(m.maxScrollStats.timeSinceView, m.lastScrollStats.timeSinceView)
}

func (m *model) refreshAfterStreamEvent() {
	if m.autoScroll || m.isListAtBottom(m.messageWidth, m.messageHeight) {
		if m.shouldRefreshNow() {
			m.refreshViewport()
			m.viewportDirty = false
			m.lastRefreshAt = time.Now()
			return
		}
	}
	m.viewportDirty = true
}

func (m *model) finalizeRun(runErr error) {
	m.busy = false
	m.escPending = false
	m.runCancel = nil
	m.stopwatch, _ = m.stopwatch.Update(stopwatch.StartStopMsg{ID: m.stopwatch.ID()})
	m.showRunResult = true
	m.queuedSteering = nil
	if runErr != nil {
		m.addItem(&session.ErrorItem{Message: runErr.Error()})
	}
	if turn := m.session.CurrentTurn(); turn != nil {
		if runErr == nil && m.currentRunKind == runKindCompact {
			m.applyCompactedContext(turn)
		}
		if m.cancelledRun {
			turn.EndWithStatus("cancelled")
		} else {
			turn.End()
		}
		m.persistTurnEnd(turn)
	}
	m.cancelledRun = false
	m.hideNextUserMessage = false
	m.currentRunKind = runKindNormal
	m.stream = nil
	m.refreshViewport()
	m.lastRefreshAt = time.Now()
	m.viewportDirty = false
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

type shellCommandDoneMsg struct {
	command string
	output  string
	err     error
}

func isShellModeInput(input string) bool {
	trimmed := strings.TrimLeft(input, " \t")
	return strings.HasPrefix(trimmed, "!")
}

func parseShellCommand(input string) string {
	trimmed := strings.TrimLeft(input, " \t")
	if !strings.HasPrefix(trimmed, "!") {
		return ""
	}
	return strings.TrimSpace(strings.TrimPrefix(trimmed, "!"))
}

func (m *model) setShellMode(enabled bool) {
	m.shellMode = enabled
	if enabled {
		applyTextareaPromptColor(&m.input, m.theme.Color(ThemeColorInputPromptShell))
		return
	}
	applyTextareaPromptColor(&m.input, m.theme.Color(ThemeColorInputPromptDefault))
}

func (m *model) shellModeActive() bool {
	return m.shellMode || isShellModeInput(m.input.Value())
}

func runShellCommandCmdWithContext(ctx context.Context, command string) tea.Cmd {
	return func() tea.Msg {
		cmd := exec.CommandContext(ctx, "bash", "-lc", command)
		// Create a new process group so we can kill all child processes on cancellation
		cmd.SysProcAttr = &syscall.SysProcAttr{
			Setpgid: true,
		}

		var stdout bytes.Buffer
		var stderr bytes.Buffer
		cmd.Stdout = &stdout
		cmd.Stderr = &stderr

		err := cmd.Start()
		if err != nil {
			return shellCommandDoneMsg{command: command, output: "", err: err}
		}

		// Wait for the process in a goroutine so we can handle cancellation
		done := make(chan error, 1)
		go func() {
			done <- cmd.Wait()
		}()

		select {
		case <-ctx.Done():
			// Context was cancelled - kill the entire process group
			if cmd.Process != nil {
				// Kill the process group (negative PID means kill the group)
				syscall.Kill(-cmd.Process.Pid, syscall.SIGKILL)
			}
			<-done // Wait for the process to actually finish
			return shellCommandDoneMsg{command: command, output: "", err: ctx.Err()}
		case err := <-done:
			output := strings.TrimRight(stdout.String(), "\n")
			stderrOutput := strings.TrimRight(stderr.String(), "\n")
			if stderrOutput != "" {
				if output != "" {
					output += "\n"
				}
				output += stderrOutput
			}

			if err != nil {
				var exitErr *exec.ExitError
				if !errors.As(err, &exitErr) {
					return shellCommandDoneMsg{command: command, output: output, err: err}
				}
			}

			return shellCommandDoneMsg{command: command, output: output}
		}
	}
}

type agentStreamStartedMsg struct {
	ch <-chan tea.Msg
}

type agentEventMsg struct {
	event     agent.Event
	btw       bool
	btwTurnID string
	btwItem   session.Item
}

type btwStreamStartedMsg struct {
	ch <-chan tea.Msg
}

type btwRunDoneMsg struct {
	turnID string
	err    error
}

type btwStreamBatchMsg struct {
	events  []agent.Event
	done    bool
	doneErr error
}

type taskSessionPlaybackTickMsg struct{}

type agentRunDoneMsg struct {
	err error
}

type streamBatchMsg struct {
	events  []agent.Event
	done    bool
	doneErr error
}

func startAgentStreamCmd(runner *agent.AgentRunner, prompt string) tea.Cmd {
	return startAgentStreamCmdWithContext(context.Background(), runner, prompt, "")
}

func startAgentStreamCmdWithContext(ctx context.Context, runner *agent.AgentRunner, prompt, internalState string) tea.Cmd {
	return func() tea.Msg {
		ch := make(chan tea.Msg)
		go func() {
			err := runner.Run(ctx, agent.Input{Content: prompt, InternalState: internalState, Type: "text"}, func(e agent.Event) {
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

func newTextareaInput() textarea.Model {
	in := textarea.New()
	in.Prompt = ""
	in.SetPromptFunc(4, func(info textarea.PromptInfo) string {
		if info.LineNumber == 0 {
			return "  > "
		}
		return " :: "
	})
	in.Placeholder = ""
	in.ShowLineNumbers = false
	in.SetHeight(inputInnerLines)
	applyTextareaPromptColor(&in, DefaultTheme().Color(ThemeColorInputPromptDefault))
	in.Focus()

	return in
}

func applyTextareaPromptColor(in *textarea.Model, promptColor lipgloss.Color) {
	styles := textarea.DefaultStyles(false)
	styles.Focused.Base = styles.Focused.Base.UnsetBackground()
	styles.Focused.Text = styles.Focused.Text.UnsetBackground()
	styles.Focused.CursorLine = styles.Focused.CursorLine.UnsetBackground()
	styles.Focused.Placeholder = styles.Focused.Placeholder.UnsetBackground()
	styles.Focused.Prompt = styles.Focused.Prompt.
		UnsetBackground().
		Foreground(promptColor).
		Bold(true)
	styles.Focused.EndOfBuffer = styles.Focused.EndOfBuffer.UnsetBackground()
	styles.Blurred.Base = styles.Blurred.Base.UnsetBackground()
	styles.Blurred.Text = styles.Blurred.Text.UnsetBackground()
	styles.Blurred.CursorLine = styles.Blurred.CursorLine.UnsetBackground()
	styles.Blurred.Placeholder = styles.Blurred.Placeholder.UnsetBackground()
	styles.Blurred.Prompt = styles.Blurred.Prompt.
		UnsetBackground().
		Foreground(promptColor).
		Bold(true)
	styles.Blurred.EndOfBuffer = styles.Blurred.EndOfBuffer.UnsetBackground()
	in.SetStyles(styles)
}

func newSessionStorage(state *session.State) *session.Storage {
	if state == nil {
		return nil
	}
	dir, err := session.DefaultStorageDir()
	if err != nil {
		fmt.Fprintf(os.Stderr, "failed to resolve session storage directory: %v\n", err)
		return nil
	}

	store, err := session.NewStorage(dir)
	if err != nil {
		fmt.Fprintf(os.Stderr, "failed to initialize session storage: %v\n", err)
		return nil
	}

	return store
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

func toolCallKey(call agent.ToolCall) string {
	if call.ID != "" {
		return call.ID
	}
	return call.Name + "|" + call.Arguments
}

const renderRefreshInterval = 33 * time.Millisecond
const scrollPriorityWindow = 120 * time.Millisecond
const streamBatchMaxEvents = 64
