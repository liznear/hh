package tui

import (
	"encoding/json"
	"fmt"
	"os"
	"reflect"
	"strings"
	"sync"
	"time"

	tea "charm.land/bubbletea/v2"
	"github.com/charmbracelet/glamour"
	glamouransi "github.com/charmbracelet/glamour/ansi"
	"github.com/charmbracelet/lipgloss"
	"github.com/charmbracelet/x/ansi"
	"github.com/liznear/hh/agent"
	"github.com/liznear/hh/tools"
	"github.com/liznear/hh/tui/session"
)

type frameViewModel struct {
	layout       layoutState
	messageList  string
	status       statusWidgetModel
	sidebarLines []string
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

	layout := m.computeLayout(m.width, m.height)
	if !layout.valid {
		return m.newAppView("")
	}
	m.syncLayoutWith(layout)

	vm := m.buildFrameViewModel(layout)
	content := m.renderFrame(vm)
	if m.debug {
		m.lastFrameBytes = len(content)
		m.maxFrameBytes = maxInt(m.maxFrameBytes, m.lastFrameBytes)
	}

	v := m.newAppView(content)
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

func (m *model) buildFrameViewModel(layout layoutState) frameViewModel {
	messageList := m.renderMessageList(layout.mainWidth, layout.messageHeight)
	if m.taskSessionView != nil {
		messageList = m.renderTaskSessionMessageList(layout.mainWidth, layout.messageHeight)
	} else if m.questionDialog != nil {
		messageList = m.renderQuestionDialog(layout.mainWidth, layout.messageHeight)
	} else if m.diffDialog != nil {
		messageList = m.renderDiffDialog(layout.mainWidth, layout.messageHeight)
	} else if m.resumePicker != nil {
		messageList = m.renderResumePickerDialog(layout.mainWidth, layout.messageHeight)
	} else if m.modelPicker != nil {
		messageList = m.renderModelPickerDialog(layout.mainWidth, layout.messageHeight)
	}

	return frameViewModel{
		layout:      layout,
		messageList: messageList,
		status: statusWidgetModel{
			AgentName:     m.agentName,
			ModelName:     m.modelName,
			Busy:          m.busy,
			ShowRunResult: m.showRunResult,
			SpinnerView:   m.spinner.View(),
			Elapsed:       m.stopwatch.Elapsed(),
			EscPending:    m.escPending,
			ShellMode:     m.shellModeActive(),
		},
		sidebarLines: m.buildSidebarLines(layout.sidebarWidth),
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
	statusBlock := lipgloss.JoinVertical(lipgloss.Left, "", "", statusLine)

	inputContent := m.input.View()
	inputBoxStyle := lipgloss.NewStyle().
		Width(layout.inputBoxWidth).
		Border(lipgloss.NormalBorder(), true, false, false, false).
		BorderForeground(m.theme.Color(ThemeColorInputBorder)).
		Height(inputInnerLines)

	if m.taskSessionView != nil {
		msg := "Task session view (read-only). Press Esc to return."
		inputContent = strings.Join(wrapLine(msg, max(1, layout.inputTextWidth-2)), "\n")
		inputBoxStyle = inputBoxStyle.Foreground(m.theme.Color(ThemeColorModelPickerMutedForeground))
	}

	popup := m.renderMentionAutocomplete(layout.inputBoxWidth)
	inputBox := inputBoxStyle.Render(inputContent)

	inputBlock := statusBlock
	if popup != "" {
		inputBlock = lipgloss.JoinVertical(lipgloss.Left, inputBlock, popup)
	}
	inputBlock = lipgloss.JoinVertical(lipgloss.Left, inputBlock, inputBox)
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

func (m *model) buildSidebarLines(sidebarWidth int) []string {
	m.refreshGitSnapshotMaybe()

	bold := lipgloss.NewStyle().Bold(true)
	warning := lipgloss.NewStyle().Foreground(m.theme.Color(ThemeColorSidebarWarningForeground))
	errorStyle := lipgloss.NewStyle().Foreground(m.theme.Color(ThemeColorSidebarErrorForeground))
	success := lipgloss.NewStyle().Foreground(m.theme.Color(ThemeColorSidebarSuccessForeground))

	title := strings.TrimSpace(m.session.Title)
	if title == "" {
		title = "Untitled Session"
	}
	title = bold.Render(title)

	wdLine := m.workingDir
	if wdLine == "" {
		wdLine = "."
	}
	wdLine = beautifySidebarPath(wdLine, os.Getenv("HOME"))
	if strings.TrimSpace(m.gitBranch) != "" {
		wdLine = fmt.Sprintf("%s @ %s", wdLine, m.gitBranch)
	}

	usedTokens := maxInt(0, m.contextWindowUsed)
	totalTokens := m.contextWindowTotalFor(strings.TrimSpace(m.modelName))
	if totalTokens <= 0 {
		totalTokens = 1
	}
	percentage := float64(usedTokens) * 100 / float64(totalTokens)
	numberStyle := lipgloss.NewStyle()
	if percentage > 50 {
		numberStyle = errorStyle
	} else if percentage > 30 {
		numberStyle = warning
	}
	contextLine := fmt.Sprintf(
		"%s %s/%s %s%%",
		bold.Render("Context Usage:"),
		numberStyle.Render(fmt.Sprintf("%d", usedTokens)),
		numberStyle.Render(fmt.Sprintf("%d", totalTokens)),
		numberStyle.Render(fmt.Sprintf("%.1f", percentage)),
	)

	doneTodos := 0
	for _, todo := range m.session.TodoItems {
		if todo.Status == session.TodoStatusCompleted {
			doneTodos++
		}
	}

	sidebarLines := []string{
		title,
		"",
		wdLine,
		"",
		contextLine,
	}
	m.sidebarModifiedFileLines = nil

	if m.questionSubmittedCount > 0 || m.questionValidationErrors > 0 {
		questionLine := fmt.Sprintf("%s %d", bold.Render("Questions answered:"), m.questionSubmittedCount)
		if m.questionValidationErrors > 0 {
			questionLine += fmt.Sprintf(" (%d errors)", m.questionValidationErrors)
		}
		sidebarLines = append(sidebarLines, "", questionLine)
		if m.questionLastLatency > 0 {
			sidebarLines = append(sidebarLines, fmt.Sprintf("%s %s", bold.Render("Last answer latency:"), formatDuration(m.questionLastLatency)))
		}
	}

	contentWidth := max(1, sidebarWidth-2)
	if len(m.modifiedFiles) > 0 {
		sidebarLines = append(sidebarLines, "", bold.Render("Modified Files"))
		for _, file := range m.modifiedFiles {
			lineNumber := len(sidebarLines)
			sidebarLines = append(sidebarLines, renderModifiedFileLine(contentWidth, file, success, errorStyle))
			m.sidebarModifiedFileLines = append(m.sidebarModifiedFileLines, sidebarModifiedFileLine{Line: lineNumber, Path: file.Path})
		}
		sidebarLines = append(sidebarLines, warning.Render("click file to open diff"))
	}

	if len(m.session.TodoItems) > 0 {
		sidebarLines = append(sidebarLines, "", bold.Render(fmt.Sprintf("TODO (%d / %d)", doneTodos, len(m.session.TodoItems))))
		for _, item := range m.session.TodoItems {
			line := fmt.Sprintf("[ ] %s", item.Content)
			switch item.Status {
			case session.TodoStatusCompleted:
				line = fmt.Sprintf("[%s] %s", success.Render("✓"), item.Content)
			case session.TodoStatusWIP:
				line = warning.Render(line)
			}
			sidebarLines = append(sidebarLines, line)
		}
	}

	if !m.debug {
		return sidebarLines
	}

	return append(sidebarLines,
		"",
		bold.Render("Debug"),
		fmt.Sprintf("Render: %s (max %s)", formatDuration(m.lastRenderLatency), formatDuration(m.maxRenderLatency)),
		fmt.Sprintf("Scroll[%s]: %s (max %s, dy=%d)", m.lastScrollStats.inputType, formatDuration(m.lastScrollStats.viewportUpdate), formatDuration(m.maxScrollStats.viewportUpdate), m.lastScrollStats.deltaRows),
		fmt.Sprintf("Scroll gap/view: %s / %s", formatDuration(m.lastScrollStats.updateGap), formatDuration(m.lastScrollStats.timeSinceView)),
		fmt.Sprintf("Scroll gap/view max: %s / %s", formatDuration(m.maxScrollStats.updateGap), formatDuration(m.maxScrollStats.timeSinceView)),
		fmt.Sprintf("Scroll->View: %s / %s", formatDuration(m.lastScrollStats.inputToViewStart), formatDuration(m.lastScrollStats.inputToViewDone)),
		fmt.Sprintf("Scroll->View max: %s / %s (events max %d)", formatDuration(m.maxScrollStats.inputToViewStart), formatDuration(m.maxScrollStats.inputToViewDone), m.maxScrollStats.coalescedEvents),
		fmt.Sprintf("Frame bytes: %d (max %d)", m.lastFrameBytes, m.maxFrameBytes),
	)
}

func renderModifiedFileLine(contentWidth int, file modifiedFileStat, addStyle lipgloss.Style, delStyle lipgloss.Style) string {
	left := displayPath(file.Path)
	parts := make([]string, 0, 2)
	if file.Added > 0 {
		parts = append(parts, addStyle.Render(fmt.Sprintf("+%d", file.Added)))
	}
	if file.Deleted > 0 {
		parts = append(parts, delStyle.Render(fmt.Sprintf("-%d", file.Deleted)))
	}
	if len(parts) == 0 {
		return left
	}

	right := strings.Join(parts, " ")
	rightWidth := lipgloss.Width(right)
	if contentWidth <= rightWidth+1 {
		return right
	}
	leftWidth := contentWidth - rightWidth - 1
	if lipgloss.Width(left) > leftWidth {
		left = truncateToWidth(left, leftWidth)
	}
	padding := strings.Repeat(" ", max(1, contentWidth-lipgloss.Width(left)-rightWidth))
	return left + padding + right
}

func (m *model) refreshGitSnapshotMaybe() {
	if m == nil {
		return
	}
	if m.workingDir == "" {
		m.workingDir = detectWorkingDirectory()
	}
	if m.lastGitRefreshAt.IsZero() || time.Since(m.lastGitRefreshAt) >= sidebarGitRefreshInterval {
		m.gitBranch = detectGitBranch(m.workingDir)
		m.modifiedFiles = collectModifiedFiles(m.workingDir)
		m.lastGitRefreshAt = time.Now()
	}
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
	line := " " + lipgloss.NewStyle().Foreground(m.theme.Color(ThemeColorSidebarSeparatorForeground)).Render("│") + " "
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

const (
	inputBoxWidthOffset  = 0
	inputTextWidthOffset = 0
)

type layoutState struct {
	valid bool

	outerWidth  int
	outerHeight int

	innerWidth  int
	innerHeight int

	showSidebar  bool
	mainWidth    int
	sidebarWidth int

	messageHeight int
	inputHeight   int

	inputBoxWidth  int
	inputTextWidth int
}

func (m *model) computeLayout(width, height int) layoutState {
	if width <= 0 || height <= 0 {
		return layoutState{}
	}

	innerW := max(1, width-(appPadding*2))
	innerH := max(1, height-(appPadding*2))
	showSidebar := width > sidebarHideWidth

	mainW := innerW
	if showSidebar {
		mainW = max(1, innerW-sidebarWidth-mainSidebarGap)
	}

	messageH, inputH := computePaneHeights(innerH)
	requiredInput := defaultInputLines + m.mentionAutocompleteHeight()
	if requiredInput > inputH {
		maxInput := max(1, innerH-1)
		if requiredInput > maxInput {
			requiredInput = maxInput
		}
		inputH = requiredInput
		messageH = max(1, innerH-inputH)
	}

	return layoutState{
		valid:          true,
		outerWidth:     width,
		outerHeight:    height,
		innerWidth:     innerW,
		innerHeight:    innerH,
		showSidebar:    showSidebar,
		mainWidth:      mainW,
		sidebarWidth:   sidebarWidth,
		messageHeight:  messageH,
		inputHeight:    inputH,
		inputBoxWidth:  max(1, mainW-inputBoxWidthOffset),
		inputTextWidth: max(1, mainW-inputTextWidthOffset),
	}
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

type statusWidgetModel struct {
	AgentName     string
	ModelName     string
	Busy          bool
	ShowRunResult bool
	SpinnerView   string
	Elapsed       time.Duration
	EscPending    bool
	ShellMode     bool
}

func renderStatusWidget(vm statusWidgetModel, theme Theme) string {
	padding := "  "
	if vm.ShellMode && !vm.Busy {
		return padding + "Shell"
	}
	base := strings.TrimSpace(vm.AgentName)
	if base == "" {
		base = "Build"
	}
	if strings.TrimSpace(vm.ModelName) != "" {
		base = fmt.Sprintf("%s · %s", base, vm.ModelName)
	}

	if vm.Busy {
		spinnerView := lipgloss.NewStyle().Foreground(theme.Color(ThemeColorStatusSpinnerForeground)).Render(vm.SpinnerView)
		durationView := lipgloss.NewStyle().Foreground(theme.Color(ThemeColorStatusDurationForeground)).Render(formatElapsedSeconds(vm.Elapsed))
		hint := ""
		if vm.EscPending {
			hint = lipgloss.NewStyle().Foreground(theme.Color(ThemeColorStatusInterruptHintForeground)).Render(" esc again to interrupt")
		}
		return fmt.Sprintf("%s%s · %s %s%s", padding, base, durationView, spinnerView, hint)
	}

	if vm.ShowRunResult {
		durationView := lipgloss.NewStyle().Foreground(theme.Color(ThemeColorStatusDurationForeground)).Render(formatElapsedSeconds(vm.Elapsed))
		return fmt.Sprintf("%s%s · %s", padding, base, durationView)
	}

	return padding + base
}

func formatElapsedSeconds(d time.Duration) string {
	if d < 0 {
		d = 0
	}
	return fmt.Sprintf("%ds", int(d.Truncate(time.Second)/time.Second))
}

func (m *model) renderUserMessageWidget(item *session.UserMessage, width int) []string {
	content := item.Content
	if item != nil && item.Queued {
		badge := lipgloss.NewStyle().
			Foreground(m.theme.Background()).
			Background(m.theme.Foreground()).
			Padding(0, 1).
			Render("Queued")
		content = badge + " " + content
	}

	// Split by newlines first, then wrap each line individually
	wrapWidth := max(1, width-3)
	var userLines []string
	for _, line := range strings.Split(content, "\n") {
		userLines = append(userLines, wrapLine(line, wrapWidth)...)
	}
	if len(userLines) == 0 {
		userLines = []string{""}
	}

	prefix := lipgloss.
		NewStyle().
		Border(lipgloss.NormalBorder(), false).
		BorderLeft(true).
		PaddingLeft(1).
		BorderLeftForeground(m.theme.Color(ThemeColorUserMessageBorderForeground))
	lines := make([]string, 0, len(userLines))
	for _, line := range userLines {
		lines = append(lines, prefix.Render(line))
	}
	return lines
}

func (m *model) renderShellMessageWidget(item *session.ShellMessage, width int) []string {
	if item == nil {
		return []string{""}
	}

	const shellBoxLeftMargin = 2
	boxWidth := max(1, width-2-shellBoxLeftMargin)
	innerWidth := max(1, boxWidth-2)

	contentLines := []string{"$ " + item.Command, ""}
	output := item.Output
	if output == "" {
		output = "(no output)"
	}
	for _, line := range strings.Split(output, "\n") {
		wrapped := wrapLine(line, innerWidth)
		if len(wrapped) == 0 {
			contentLines = append(contentLines, "")
			continue
		}
		contentLines = append(contentLines, wrapped...)
	}

	box := lipgloss.NewStyle().
		Background(m.theme.Color(ThemeColorShellMessageBackground)).
		Padding(1).
		MarginLeft(shellBoxLeftMargin).
		Width(boxWidth).
		Render(strings.Join(contentLines, "\n"))

	return strings.Split(box, "\n")
}

func (m *model) renderAssistantMessageWidget(item *session.AssistantMessage, width int) []string {
	renderedMarkdown := RenderMarkdown(item.Content, max(1, width-2))
	if renderedMarkdown == "" {
		return []string{""}
	}
	return strings.Split(renderedMarkdown, "\n")
}

func (m *model) renderThinkingWidget(item *session.ThinkingBlock, width int) []string {
	renderedMarkdown := RenderMarkdown(item.Content, max(1, width-2), ThinkingOption())
	if renderedMarkdown == "" {
		return []string{""}
	}
	return strings.Split(renderedMarkdown, "\n")
}

func (m *model) renderTurnFooterWidget(modelName string, duration time.Duration, status string, width int) []string {
	bodyWidth := max(1, width-2)
	muted := lipgloss.NewStyle().Foreground(m.theme.Color(ThemeColorTurnFooterForeground))

	statusLabel := ""
	if strings.EqualFold(status, "cancelled") {
		statusLabel = " Cancelled"
	}
	meta := strings.TrimSpace(fmt.Sprintf("◆ %s %s%s", modelName, formatElapsedSeconds(duration), statusLabel))
	if ansi.StringWidth(meta) >= bodyWidth {
		return []string{"  " + muted.Render(truncateToWidth(meta, bodyWidth))}
	}

	ruleWidth := bodyWidth - ansi.StringWidth(meta) - 1
	rule := strings.Repeat("─", max(0, ruleWidth))
	line := strings.TrimSpace(meta + " " + rule)
	return []string{"  " + muted.Render(line)}
}

func truncateToWidth(s string, maxWidth int) string {
	if maxWidth <= 0 {
		return ""
	}
	if ansi.StringWidth(s) <= maxWidth {
		return s
	}
	if maxWidth == 1 {
		return "…"
	}

	target := maxWidth - 1
	var b strings.Builder
	width := 0
	for _, r := range s {
		rw := ansi.StringWidth(string(r))
		if width+rw > target {
			break
		}
		b.WriteRune(r)
		width += rw
	}
	return b.String() + "…"
}

type styledToken struct {
	raw   string
	style lipgloss.Style
}

type toolCallWidgetModel struct {
	Item       *session.ToolCallItem
	Width      int
	WorkingDir string
}

func (m *model) renderToolCallWidget(item *session.ToolCallItem, width int) []string {
	vm := toolCallWidgetModel{Item: item, Width: max(1, width-2), WorkingDir: m.workingDir}
	toolLines := renderToolCallWidget(vm, m.theme)
	return prefixedLines(toolLines, "  ")
}

func renderToolCallWidget(vm toolCallWidgetModel, theme Theme) []string {
	if vm.Item == nil {
		return nil
	}
	isTaskTool := strings.ToLower(vm.Item.Name) == "task"

	body, tokens := formatToolCallWidgetBody(vm, theme)
	bodyWidth := max(1, vm.Width-2)
	bodyLines := make([]string, 0)
	for _, paragraph := range strings.Split(body, "\n") {
		wrapped := wrapLine(paragraph, bodyWidth)
		if len(wrapped) == 0 {
			bodyLines = append(bodyLines, "")
			continue
		}
		bodyLines = append(bodyLines, wrapped...)
	}
	if len(bodyLines) == 0 {
		return []string{renderToolCallIcon(vm, theme)}
	}

	out := make([]string, 0, len(bodyLines))
	for i, line := range bodyLines {
		styledLine := styleToolCallLine(line, tokens)
		if isTaskTool {
			out = append(out, styledLine)
			continue
		}
		if i == 0 {
			out = append(out, fmt.Sprintf("%s %s", renderToolCallIcon(vm, theme), styledLine))
			continue
		}
		out = append(out, "  "+styledLine)
	}

	if diffLines := renderEditToolCallDiff(vm, theme); len(diffLines) > 0 {
		out = append(out, diffLines...)
	}

	return out
}

func renderToolCallIcon(vm toolCallWidgetModel, theme Theme) string {
	if vm.Item == nil {
		return ""
	}

	switch vm.Item.Status {
	case session.ToolCallStatusPending:
		return "→"
	case session.ToolCallStatusSuccess:
		return lipgloss.NewStyle().Foreground(theme.Color(ThemeColorToolCallIconSuccessForeground)).Render("✓")
	default:
		return lipgloss.NewStyle().Foreground(theme.Color(ThemeColorToolCallIconErrorForeground)).Render("⨯")
	}
}

func renderEditToolCallDiff(vm toolCallWidgetModel, theme Theme) []string {
	item := vm.Item
	if item == nil || item.Status != session.ToolCallStatusSuccess || strings.ToLower(item.Name) != "edit" || item.Result == nil {
		return nil
	}
	editResult, ok := item.Result.Result.(tools.EditResult)
	if !ok {
		return nil
	}
	if editResult.OldContent == editResult.NewContent {
		return nil
	}

	diffWidth := max(20, vm.Width-2)
	diffLines := RenderSplitDiff(editResult.OldContent, editResult.NewContent, editResult.Path, diffWidth, theme)
	out := make([]string, 0, len(diffLines)+1)
	out = append(out, "")
	for _, line := range diffLines {
		out = append(out, "  "+line)
	}
	return out
}

func formatToolCallWidgetBody(vm toolCallWidgetModel, theme Theme) (string, []styledToken) {
	item := vm.Item
	if item == nil {
		return "", nil
	}

	args := parseToolCallArgs(item.Arguments)

	pathStyle := lipgloss.NewStyle().Foreground(theme.Color(ThemeColorToolCallPathForeground))
	addStyle := lipgloss.NewStyle().Foreground(theme.Color(ThemeColorToolCallAddForeground))
	delStyle := lipgloss.NewStyle().Foreground(theme.Color(ThemeColorToolCallDeleteForeground))

	switch strings.ToLower(item.Name) {
	case "list", "ls":
		path := beautifyToolPath(toolArgString(args, "path", "."), vm.WorkingDir)
		body := fmt.Sprintf("List %s", path)
		if item.Status == session.ToolCallStatusSuccess {
			if files, ok := listFileCount(item); ok {
				body = fmt.Sprintf("%s (%d files)", body, files)
			}
		}
		return body, []styledToken{{raw: path, style: pathStyle}}

	case "read":
		path := beautifyToolPath(toolArgString(args, "path", "."), vm.WorkingDir)
		body := fmt.Sprintf("Read %s", path)
		return body, []styledToken{{raw: path, style: pathStyle}}

	case "grep":
		path := beautifyToolPath(toolArgString(args, "path", "."), vm.WorkingDir)
		body := fmt.Sprintf("Grep %s", path)
		if item.Status == session.ToolCallStatusSuccess {
			if matches, ok := grepMatchCount(item); ok {
				body = fmt.Sprintf("%s (%d matches)", body, matches)
			}
		}
		return body, []styledToken{{raw: path, style: pathStyle}}

	case "edit":
		path := beautifyToolPath(toolArgString(args, "path", "."), vm.WorkingDir)
		body := fmt.Sprintf("Edit %s", path)
		tokens := []styledToken{{raw: path, style: pathStyle}}
		if item.Status == session.ToolCallStatusSuccess {
			if added, deleted, ok := editCounts(item); ok {
				addToken := fmt.Sprintf("+%d", added)
				delToken := fmt.Sprintf("-%d", deleted)
				body = fmt.Sprintf("%s %s %s", body, addToken, delToken)
				tokens = append(tokens,
					styledToken{raw: addToken, style: addStyle},
					styledToken{raw: delToken, style: delStyle},
				)
			}
		}
		return body, tokens

	case "write":
		path := beautifyToolPath(toolArgString(args, "path", "."), vm.WorkingDir)
		body := fmt.Sprintf("Write %s", path)
		tokens := []styledToken{{raw: path, style: pathStyle}}
		if item.Status == session.ToolCallStatusSuccess {
			if added, ok := writeAddedLines(item); ok {
				addToken := fmt.Sprintf("+%d", added)
				body = fmt.Sprintf("%s %s", body, addToken)
				tokens = append(tokens, styledToken{raw: addToken, style: addStyle})
			}
		}
		return body, tokens

	case "web_search":
		query := toolArgString(args, "query", "")
		body := fmt.Sprintf("WebSearch %q", query)
		return body, []styledToken{{raw: query, style: pathStyle}}

	case "web_fetch":
		url := toolArgString(args, "url", "")
		body := fmt.Sprintf("WebFetch %q", url)
		return body, []styledToken{{raw: url, style: pathStyle}}

	case "glob":
		pattern := toolArgString(args, "pattern", "*")
		path := beautifyToolPath(toolArgString(args, "path", "."), vm.WorkingDir)
		body := fmt.Sprintf("Glob %q in %s", pattern, path)
		tokens := []styledToken{{raw: pattern, style: pathStyle}, {raw: path, style: pathStyle}}
		if item.Status == session.ToolCallStatusSuccess {
			if matches, ok := globMatchCount(item); ok {
				body = fmt.Sprintf("%s (%d matches)", body, matches)
			}
		}
		return body, tokens

	case "bash":
		command := toolArgString(args, "command", "")
		if command == "" {
			return "Bash", nil
		}
		commandMaxLen := min(50, vm.Width-20)
		displayCommand := truncateToolCommand(command, commandMaxLen)
		body := fmt.Sprintf("Bash %q", displayCommand)
		return body, []styledToken{{raw: displayCommand, style: pathStyle}}

	case "task":
		successStyle := lipgloss.NewStyle().Foreground(theme.Color(ThemeColorToolCallIconSuccessForeground))
		errorStyle := lipgloss.NewStyle().Foreground(theme.Color(ThemeColorToolCallIconErrorForeground))
		if lines, tokens := formatTaskToolCallLines(item, args, pathStyle, successStyle, errorStyle, vm.Width); len(lines) > 0 {
			return strings.Join(lines, "\n"), tokens
		}
		return "Task", nil

	case "todo_write":
		done, total, ok := todoProgress(item, args)
		if ok {
			return fmt.Sprintf("TODO %d / %d", done, total), nil
		}
		return "TODO", nil

	case "question":
		title := questionTitleArg(args)
		if title == "" {
			return `Question: ""`, nil
		}
		return fmt.Sprintf("Question: %q", title), []styledToken{{raw: title, style: pathStyle}}

	case "skill":
		skillName := toolArgString(args, "name", "")
		if skillName == "" && item.Result != nil {
			if result, ok := item.Result.Result.(tools.SkillResult); ok {
				skillName = result.Name
			}
		}
		if skillName == "" {
			return "Skill", nil
		}
		return fmt.Sprintf("Skill %q", skillName), []styledToken{{raw: skillName, style: pathStyle}}

	default:
		return formatGenericToolCallWidgetBody(item)
	}
}

func formatTaskToolCallLines(item *session.ToolCallItem, args map[string]any, pathStyle lipgloss.Style, successStyle lipgloss.Style, errorStyle lipgloss.Style, width int) ([]string, []styledToken) {
	requests := taskLineRequests(item, args)
	if len(requests) == 0 {
		return nil, nil
	}

	maxTaskLen := max(20, width-30)
	lines := make([]string, 0, len(requests))
	tokens := make([]styledToken, 0, len(requests)*4)
	for _, req := range requests {
		statusPrefix := "•"
		statusStyle := pathStyle
		switch req.Status {
		case "success":
			statusPrefix = "✓"
			statusStyle = successStyle
		case "error":
			statusPrefix = "⨯"
			statusStyle = errorStyle
		}

		displayTask := truncateToolCommand(req.Task, maxTaskLen)
		line := fmt.Sprintf("%s Task %s: %s", statusPrefix, req.SubAgentName, displayTask)
		lines = append(lines, line)
		tokens = append(tokens,
			styledToken{raw: statusPrefix, style: statusStyle},
			styledToken{raw: req.SubAgentName, style: pathStyle},
			styledToken{raw: displayTask, style: pathStyle},
		)
		if req.Status == "error" && req.Error != "" {
			lines = append(lines, "  |- "+req.Error)
			tokens = append(tokens, styledToken{raw: req.Error, style: errorStyle})
		}
	}
	return lines, tokens
}

func taskLineRequests(item *session.ToolCallItem, args map[string]any) []taskLineRequest {
	requests := make([]taskLineRequest, 0)

	if item != nil && item.Result != nil {
		if result, ok := item.Result.Result.(tools.TaskResult); ok {
			for idx, task := range result.Tasks {
				requests = append(requests, taskLineRequest{
					ParentToolCallID: strings.TrimSpace(item.ID),
					TaskIndex:        idx,
					SubAgentName:     task.SubAgentName,
					Task:             task.Task,
					Status:           string(task.Status),
					Error:            task.Error,
					AgentMessages:    append([]agent.Message(nil), task.Messages...),
				})
			}
		}
	}

	if len(requests) == 0 {
		if rawTasks, ok := args["tasks"]; ok && rawTasks != nil {
			if items, ok := rawTasks.([]any); ok {
				for idx, raw := range items {
					itemMap, ok := raw.(map[string]any)
					if !ok {
						continue
					}
					if req, ok := parseTaskLineRequest(itemMap); ok {
						req.TaskIndex = idx
						if item != nil {
							req.ParentToolCallID = strings.TrimSpace(item.ID)
						}
						requests = append(requests, req)
					}
				}
			}
		}
	}

	return requests
}

type taskLineRequest struct {
	ParentToolCallID string
	TaskIndex        int
	SubAgentName     string
	Task             string
	Status           string
	Error            string
	AgentMessages    []agent.Message
}

func parseTaskLineRequest(raw map[string]any) (taskLineRequest, bool) {
	subAgent, ok := raw["sub_agent_name"].(string)
	if !ok || strings.TrimSpace(subAgent) == "" {
		return taskLineRequest{}, false
	}
	task, ok := raw["task"].(string)
	if !ok || strings.TrimSpace(task) == "" {
		return taskLineRequest{}, false
	}
	status, _ := raw["status"].(string)
	errMessage, _ := raw["error"].(string)
	return taskLineRequest{
		SubAgentName: strings.TrimSpace(subAgent),
		Task:         strings.TrimSpace(task),
		Status:       strings.TrimSpace(status),
		Error:        strings.TrimSpace(errMessage),
	}, true
}

func questionTitleArg(args map[string]any) string {
	rawQuestion, ok := args["question"]
	if !ok || rawQuestion == nil {
		return ""
	}
	question, ok := rawQuestion.(map[string]any)
	if !ok {
		return ""
	}
	title, ok := question["title"].(string)
	if !ok {
		return ""
	}
	return strings.TrimSpace(title)
}

func styleToolCallLine(line string, tokens []styledToken) string {
	styled := line
	for _, token := range tokens {
		if token.raw == "" {
			continue
		}
		idx := strings.Index(styled, token.raw)
		if idx < 0 {
			continue
		}
		replacement := token.style.Render(token.raw)
		styled = styled[:idx] + replacement + styled[idx+len(token.raw):]
	}
	return styled
}

func parseToolCallArgs(raw string) map[string]any {
	raw = strings.TrimSpace(raw)
	if raw == "" || raw == "{}" {
		return map[string]any{}
	}

	var out map[string]any
	if err := json.Unmarshal([]byte(raw), &out); err != nil {
		return map[string]any{}
	}
	return out
}

func toolArgString(args map[string]any, key string, fallback string) string {
	v, ok := args[key]
	if !ok || v == nil {
		return fallback
	}
	s, ok := v.(string)
	if !ok || strings.TrimSpace(s) == "" {
		return fallback
	}
	return s
}

func listFileCount(item *session.ToolCallItem) (int, bool) {
	if item == nil || item.Result == nil {
		return 0, false
	}
	if result, ok := item.Result.Result.(tools.ListResult); ok {
		return result.FileCount, true
	}
	return 0, false
}

func grepMatchCount(item *session.ToolCallItem) (int, bool) {
	if item == nil || item.Result == nil {
		return 0, false
	}
	if result, ok := item.Result.Result.(tools.GrepResult); ok {
		return result.MatchCount, true
	}
	return 0, false
}

func globMatchCount(item *session.ToolCallItem) (int, bool) {
	if item == nil || item.Result == nil {
		return 0, false
	}
	if result, ok := item.Result.Result.(tools.GlobResult); ok {
		return result.MatchCount, true
	}
	return 0, false
}

func editCounts(item *session.ToolCallItem) (int, int, bool) {
	if item == nil || item.Result == nil {
		return 0, 0, false
	}
	if result, ok := item.Result.Result.(tools.EditResult); ok {
		return result.AddedLines, result.DeletedLines, true
	}
	return 0, 0, false
}

func writeAddedLines(item *session.ToolCallItem) (int, bool) {
	if item == nil || item.Result == nil {
		return 0, false
	}
	if result, ok := item.Result.Result.(tools.WriteResult); ok {
		return result.AddedLines, true
	}
	return 0, false
}

func todoProgress(item *session.ToolCallItem, args map[string]any) (int, int, bool) {
	if done, total, ok := todoProgressFromResult(item); ok {
		return done, total, true
	}
	return todoProgressFromArgs(args)
}

func todoProgressFromResult(item *session.ToolCallItem) (int, int, bool) {
	if item == nil || item.Result == nil {
		return 0, 0, false
	}

	var todoItems []tools.TodoItem
	switch result := item.Result.Result.(type) {
	case tools.TodoWriteResult:
		todoItems = result.TodoItems
	case *tools.TodoWriteResult:
		if result == nil {
			return 0, 0, false
		}
		todoItems = result.TodoItems
	default:
		return 0, 0, false
	}

	done := 0
	for _, todo := range todoItems {
		status := strings.ToLower(strings.TrimSpace(string(todo.Status)))
		if status == string(session.TodoStatusCompleted) || status == string(session.TodoStatusCancelled) {
			done++
		}
	}
	return done, len(todoItems), true
}

func todoProgressFromArgs(args map[string]any) (int, int, bool) {
	raw, ok := args["todo_items"]
	if !ok || raw == nil {
		return 0, 0, false
	}

	todoItems, ok := raw.([]any)
	if !ok {
		return 0, 0, false
	}

	done := 0
	for _, rawItem := range todoItems {
		itemMap, ok := rawItem.(map[string]any)
		if !ok {
			continue
		}
		status, ok := itemMap["status"].(string)
		if !ok {
			continue
		}
		normalized := strings.ToLower(strings.TrimSpace(status))
		if normalized == string(session.TodoStatusCompleted) || normalized == string(session.TodoStatusCancelled) {
			done++
		}
	}

	return done, len(todoItems), true
}

func formatGenericToolCallWidgetBody(item *session.ToolCallItem) (string, []styledToken) {
	args := strings.TrimSpace(item.Arguments)
	if args == "" || args == "{}" {
		return item.Name, nil
	}

	const maxArgLen = 80
	runes := []rune(args)
	if len(runes) > maxArgLen {
		args = string(runes[:maxArgLen-1]) + "…"
	}

	return fmt.Sprintf("%s %s", item.Name, args), nil
}

func truncateToolCommand(command string, maxLen int) string {
	if maxLen <= 0 {
		return ""
	}

	runes := []rune(command)
	if len(runes) <= maxLen {
		return command
	}

	if maxLen <= 3 {
		return strings.Repeat(".", maxLen)
	}

	return string(runes[:maxLen-3]) + "..."
}

func (m *model) renderErrorWidget(item *session.ErrorItem, width int) []string {
	return prefixedLines(wrapLine("error: "+item.Message, max(1, width-2)), "  ")
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
	// Don't cache BTWExchange items as they stream in
	if _, ok := item.(*session.BTWExchange); ok {
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
	// Don't cache BTWExchange items as they stream in
	if _, ok := item.(*session.BTWExchange); ok {
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
	case *session.ShellMessage:
		return "shell:" + v.Command + "\n" + v.Output, true
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
	case *session.BTWExchange:
		return "btw:" + v.Question + "\n" + v.Answer, true
	default:
		return "", false
	}
}

func cloneStringSlice(in []string) []string {
	out := make([]string, len(in))
	copy(out, in)
	return out
}

type markdownRenderOption struct {
	name  string
	apply func(*glamouransi.StyleConfig)
}

func ThinkingOption() markdownRenderOption {
	return markdownRenderOption{
		name: "thinking",
		apply: func(style *glamouransi.StyleConfig) {
			*style = mutedStyleConfig(*style, 0.45)
			docMargin := uint(4)
			style.Document.Margin = &docMargin
			registerCodeBlockTheme(thinkingCodeBlockThemeName, style.CodeBlock.Chroma)
			style.CodeBlock.Theme = thinkingCodeBlockThemeName
			style.CodeBlock.Chroma = nil
		},
	}
}

func RenderMarkdown(content string, width int, opts ...markdownRenderOption) string {
	if strings.TrimSpace(content) == "" {
		return ""
	}

	renderer := getMarkdownRenderer(width, opts...)
	rendered, err := renderer.Render(content)
	if err != nil {
		return strings.Join(wrapLine(content, width), "\n")
	}

	return strings.Trim(rendered, "\n")
}

var (
	markdownRenderersMu sync.Mutex
	markdownRenderers   = map[markdownRendererKey]*glamour.TermRenderer{}
)

type markdownRendererKey struct {
	width      int
	optionName string
}

func getMarkdownRenderer(width int, opts ...markdownRenderOption) *glamour.TermRenderer {
	wrapWidth := max(20, width)
	key := markdownRendererKey{width: wrapWidth, optionName: markdownOptionKey(opts)}

	markdownRenderersMu.Lock()
	defer markdownRenderersMu.Unlock()

	if renderer, ok := markdownRenderers[key]; ok {
		return renderer
	}

	style := *glamour.DefaultStyles["light"]
	if style.Document.Margin != nil {
		*style.Document.Margin = 2
	}
	for _, opt := range opts {
		if opt.apply == nil {
			continue
		}
		opt.apply(&style)
	}

	r, err := glamour.NewTermRenderer(
		glamour.WithPreservedNewLines(),
		glamour.WithWordWrap(wrapWidth),
		glamour.WithStyles(style),
	)
	if err != nil {
		panic(err)
	}
	markdownRenderers[key] = r
	return r
}

func markdownOptionKey(opts []markdownRenderOption) string {
	if len(opts) == 0 {
		return ""
	}

	parts := make([]string, 0, len(opts))
	for i, opt := range opts {
		name := strings.TrimSpace(opt.name)
		if name == "" {
			name = fmt.Sprintf("unnamed-%d", i)
		}
		parts = append(parts, name)
	}
	return strings.Join(parts, "|")
}
