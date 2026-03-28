package tui

import (
	"strings"
	"time"

	tea "charm.land/bubbletea/v2"
	"github.com/charmbracelet/lipgloss"
	"github.com/charmbracelet/x/ansi"
	"github.com/liznear/hh/tui/session"
)

func (m *model) syncLayout() {
	layout := m.computeLayout(m.width, m.height)
	m.syncLayoutWith(layout)
}

func (m *model) syncLayoutWith(layout layoutState) {
	if !layout.valid {
		return
	}

	wasAtBottom := m.isListAtBottom(m.messageWidth, m.messageHeight)
	m.messageWidth = layout.mainWidth
	m.messageHeight = layout.messageHeight
	if m.autoScroll || wasAtBottom {
		m.scrollListToBottom(m.messageWidth, m.messageHeight)
	} else {
		m.clampListOffset(m.messageWidth, m.messageHeight)
	}
	m.input.SetWidth(layout.inputTextWidth)
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
	items := m.displayItems()
	if len(items) == 0 {
		return ""
	}

	m.clampListOffset(width, height)

	visible := make([]string, 0, height)
	idx := m.listOffsetIdx
	offset := m.listOffsetLine

	for len(visible) < height && idx < len(items) {
		lines := m.renderItemLinesAt(items, idx, width)
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

func (m *model) displayItems() []session.Item {
	base := m.session.AllItems()
	if len(m.ephemeralItems) == 0 && len(m.queuedSteering) == 0 {
		return base
	}

	// Build a map of ephemeral items keyed by turnID:afterIndex
	type epKey struct {
		turnID     string
		afterIndex int
	}
	ephemeralMap := make(map[epKey][]session.Item)
	for _, ep := range m.ephemeralItems {
		k := epKey{turnID: ep.turnID, afterIndex: ep.afterIndex}
		ephemeralMap[k] = append(ephemeralMap[k], ep.item)
	}

	// Iterate through turns and items, inserting ephemeral items at the right spots
	out := make([]session.Item, 0, len(base)+len(m.ephemeralItems))
	for _, turn := range m.session.Turns {
		if turn == nil {
			continue
		}
		for i, item := range turn.Items {
			out = append(out, item)
			// Check for ephemeral items after this index
			k := epKey{turnID: turn.ID, afterIndex: i}
			if eps, ok := ephemeralMap[k]; ok {
				out = append(out, eps...)
			}
		}
	}

	// Append queued steering messages at the end
	for _, queued := range m.queuedSteering {
		out = append(out, &session.UserMessage{Content: queued.Content, Queued: true})
	}
	return out
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
	items := m.displayItems()
	if len(items) == 0 || width <= 0 || height <= 0 {
		return true
	}
	total := 0
	for i := m.listOffsetIdx; i < len(items); i++ {
		total += len(m.renderItemLinesAt(items, i, width))
		if total > height+m.listOffsetLine {
			return false
		}
	}
	return total-m.listOffsetLine <= height
}

func (m *model) clampListOffset(width, height int) {
	items := m.displayItems()
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
	items := m.displayItems()
	if len(items) == 0 {
		return 0, 0
	}
	total := 0
	idx := len(items) - 1
	for ; idx >= 0; idx-- {
		total += len(m.renderItemLinesAt(items, idx, width))
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
	items := m.displayItems()
	if len(items) == 0 {
		return
	}
	if lines > 0 {
		m.listOffsetLine += lines
		for m.listOffsetIdx < len(items) {
			itemHeight := len(m.renderItemLinesAt(items, m.listOffsetIdx, width))
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
		itemHeight := len(m.renderItemLinesAt(items, m.listOffsetIdx, width))
		if itemHeight <= 0 {
			itemHeight = 1
		}
		m.listOffsetLine += itemHeight
	}
}

func (m *model) currentListOffset(width int) int {
	items := m.displayItems()
	if len(items) == 0 || width <= 0 {
		return 0
	}
	offset := 0
	maxIdx := min(m.listOffsetIdx, len(items))
	for i := 0; i < maxIdx; i++ {
		offset += len(m.renderItemLinesAt(items, i, width))
	}
	offset += m.listOffsetLine
	return offset
}

func (m *model) renderItemLinesAt(items []session.Item, idx int, width int) []string {
	if idx < 0 || idx >= len(items) {
		return []string{""}
	}

	if _, ok := items[idx].(*session.End); ok {
		modelName, duration, status := turnFooterMeta(items, idx, m.modelName)
		footer := m.renderTurnFooterWidget(modelName, duration, status, width)
		return append([]string{""}, footer...)
	}

	lines := m.renderItemLines(items[idx], width)
	if idx <= 0 || !needsSpacerBetweenItems(items[idx-1], items[idx]) {
		return lines
	}

	out := make([]string, 0, len(lines)+1)
	out = append(out, "")
	out = append(out, lines...)
	return out
}

func turnFooterMeta(items []session.Item, endIdx int, fallbackModel string) (string, time.Duration, string) {
	modelName := fallbackModel
	if strings.TrimSpace(modelName) == "" {
		modelName = "unknown"
	}
	status := ""

	if endIdx < 0 || endIdx >= len(items) {
		return modelName, 0, status
	}

	end, ok := items[endIdx].(*session.End)
	if !ok {
		return modelName, 0, status
	}
	status = end.Status

	endTs := end.Timestamp()
	for i := endIdx - 1; i >= 0; i-- {
		switch item := items[i].(type) {
		case *session.Start:
			if strings.TrimSpace(item.Model) != "" {
				modelName = item.Model
			}
			startTs := item.Timestamp()
			if startTs.IsZero() || endTs.IsZero() || endTs.Before(startTs) {
				return modelName, 0, status
			}
			return modelName, endTs.Sub(startTs), status
		case *session.End:
			return modelName, 0, status
		}
	}

	return modelName, 0, status
}

func needsSpacerBetweenItems(prev session.Item, curr session.Item) bool {
	if !isMessageBlock(prev) || !isMessageBlock(curr) {
		return false
	}
	if prev.Type() == session.ItemTypeToolCall && curr.Type() == session.ItemTypeToolCall {
		return false
	}
	return true
}

func isMessageBlock(item session.Item) bool {
	if item == nil {
		return false
	}

	switch item.Type() {
	case session.ItemTypeUserMessage,
		session.ItemTypeShellMessage,
		session.ItemTypeAssistantMessage,
		session.ItemTypeThinkingBlock,
		session.ItemTypeToolCall,
		session.ItemTypeError,
		session.ItemTypeBTWExchange:
		return true
	default:
		return false
	}
}

func (m *model) renderItemLines(item session.Item, width int) []string {
	if cached, ok := m.getCachedRenderedItem(item, width); ok {
		return cached
	}

	var lines []string
	needsNormalize := true
	switch v := item.(type) {
	case *session.UserMessage:
		lines = m.renderUserMessageWidget(v, width)

	case *session.ShellMessage:
		lines = m.renderShellMessageWidget(v, width)

	case *session.AssistantMessage:
		lines = m.renderAssistantMessageWidget(v, width)

	case *session.ThinkingBlock:
		lines = m.renderThinkingWidget(v, width)
		needsNormalize = false

	case *session.ToolCallItem:
		lines = m.renderToolCallWidget(v, width)

	case *session.ErrorItem:
		lines = m.renderErrorWidget(v, width)

	case *session.BTWExchange:
		lines = m.renderBTWExchangeWidget(v, width)

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

func (m *model) renderBTWExchangeWidget(item *session.BTWExchange, width int) []string {
	if item == nil {
		return []string{""}
	}

	const btwBoxLeftMargin = 2
	// Account for: left margin (2) + border (2) + padding (2) = 6
	innerWidth := max(1, width-6)

	badge := lipgloss.NewStyle().
		Foreground(m.theme.Color(ThemeColorUserMessageBorderForeground)).
		Render("[btw]")

	badgePlain := "[btw] "
	badgeWidth := ansi.StringWidth(badgePlain)

	var contentLines []string

	// Question - first line has badge, subsequent lines are indented
	questionInnerWidth := max(1, innerWidth-badgeWidth)
	questionLines := wrapLine(item.Question, questionInnerWidth)
	contentLines = append(contentLines, badge+" "+questionLines[0])
	indent := strings.Repeat(" ", badgeWidth)
	for i := 1; i < len(questionLines); i++ {
		contentLines = append(contentLines, indent+questionLines[i])
	}

	// Answer
	if item.Answer != "" {
		contentLines = append(contentLines, "", renderMarkdown(item.Answer, innerWidth, ThinkingOption()))
	} else if m.btwBusy {
		// Show processing indicator
		contentLines = append(contentLines, "")
		spinner := m.spinner.View()
		contentLines = append(contentLines, spinner)
	}

	// Render all content as a single box
	box := lipgloss.NewStyle().
		Border(lipgloss.RoundedBorder()).
		BorderForeground(m.theme.Color(ThemeColorInputBorder)).
		Padding(0, 1).
		MarginLeft(btwBoxLeftMargin).
		Width(innerWidth).
		Render(strings.Join(contentLines, "\n"))

	return strings.Split(box, "\n")
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
