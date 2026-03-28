package tui

import (
	"fmt"
	"strconv"
	"strings"

	"github.com/alecthomas/chroma/v2"
	"github.com/alecthomas/chroma/v2/lexers"
	"github.com/aymanbagabas/go-udiff"
	"github.com/charmbracelet/lipgloss"
	"github.com/charmbracelet/x/ansi"
)

// DiffViewMode represents the diff display mode.
type DiffViewMode int

const (
	DiffViewUnified DiffViewMode = iota
	DiffViewSplit
)

// splitLine represents a line in a split diff view with optional before/after content.
type splitLine struct {
	before    udiff.Line
	after     udiff.Line
	hasBefore bool
	hasAfter  bool
}

// splitHunk represents a hunk in a split diff view.
type splitHunk struct {
	fromLine int
	toLine   int
	header   string
	lines    []splitLine
}

// hunkToSplit converts a unified diff hunk to a split hunk.
func hunkToSplit(h *udiff.Hunk) splitHunk {
	sh := splitHunk{
		fromLine: h.FromLine,
		toLine:   h.ToLine,
		header:   formatHunkHeader(h),
		lines:    make([]splitLine, 0, len(h.Lines)),
	}

	lines := h.Lines
	for i := 0; i < len(lines); i++ {
		ul := lines[i]
		sl := splitLine{}

		switch ul.Kind {
		case udiff.Equal:
			sl.before = ul
			sl.after = ul
			sl.hasBefore = true
			sl.hasAfter = true

		case udiff.Insert:
			sl.after = ul
			sl.hasAfter = true

		case udiff.Delete:
			sl.before = ul
			sl.hasBefore = true
			// Look ahead for matching insert
			for j := i + 1; j < len(lines); j++ {
				if lines[j].Kind == udiff.Insert {
					sl.after = lines[j]
					sl.hasAfter = true
					// Remove the matched insert from lines
					lines = append(lines[:j], lines[j+1:]...)
					break
				} else if lines[j].Kind == udiff.Equal {
					break
				}
			}
		}

		sh.lines = append(sh.lines, sl)
	}

	return sh
}

func formatHunkHeader(h *udiff.Hunk) string {
	beforeLines, afterLines := countHunkLines(h)
	return fmt.Sprintf("@@ -%d,%d +%d,%d @@", h.FromLine, beforeLines, h.ToLine, afterLines)
}

func countHunkLines(h *udiff.Hunk) (before, after int) {
	for _, l := range h.Lines {
		switch l.Kind {
		case udiff.Equal:
			before++
			after++
		case udiff.Insert:
			after++
		case udiff.Delete:
			before++
		}
	}
	return
}

// lineStyle defines colors for a line type in the diff view.
type lineStyle struct {
	lineNumBg lipgloss.Color
	lineNumFg lipgloss.Color
	symbolBg  lipgloss.Color
	symbolFg  lipgloss.Color
	codeBg    lipgloss.Color
	codeFg    lipgloss.Color
}

// RenderSplitDiff renders a split (side-by-side) diff view.
func RenderSplitDiff(oldContent, newContent, filePath string, width int, theme Theme) []string {
	if oldContent == newContent {
		return []string{"(no changes)"}
	}

	edits := udiff.Lines(oldContent, newContent)
	unified, err := udiff.ToUnifiedDiff(filePath, filePath, oldContent, edits, 3)
	if err != nil || len(unified.Hunks) == 0 {
		return []string{"(no diff)"}
	}

	// Convert to split hunks
	splitHunks := make([]splitHunk, len(unified.Hunks))
	for i, h := range unified.Hunks {
		splitHunks[i] = hunkToSplit(h)
	}

	// Calculate dimensions
	sepWidth := 3 // " x " where x is the vertical bar
	columnWidth := max(20, (width-sepWidth)/2)

	// Find max line number for padding
	maxLine := 0
	for _, sh := range splitHunks {
		maxLine = max(maxLine, sh.fromLine+len(sh.lines))
		maxLine = max(maxLine, sh.toLine+len(sh.lines))
	}
	numWidth := max(2, len(strconv.Itoa(maxLine)))

	// Styles - using light backgrounds for visibility
	fg := lipgloss.Color("#1a1a1a") // dark text for light backgrounds

	// Delete line style (very light red background)
	deleteStyle := lineStyle{
		lineNumBg: lipgloss.Color("#ffebee"), // very light red
		lineNumFg: lipgloss.Color("#c62828"), // dark red
		symbolBg:  lipgloss.Color("#ffebee"),
		symbolFg:  lipgloss.Color("#c62828"),
		codeBg:    lipgloss.Color("#ffebee"),
		codeFg:    fg,
	}

	// Insert line style (very light green background)
	insertStyle := lineStyle{
		lineNumBg: lipgloss.Color("#e8f5e9"), // very light green
		lineNumFg: lipgloss.Color("#2e7d32"), // dark green
		symbolBg:  lipgloss.Color("#e8f5e9"),
		symbolFg:  lipgloss.Color("#2e7d32"),
		codeBg:    lipgloss.Color("#e8f5e9"),
		codeFg:    fg,
	}

	// Equal/context line style (no special background)
	equalStyle := lineStyle{
		lineNumBg: lipgloss.Color(""),
		lineNumFg: lipgloss.Color("8"), // muted
		symbolBg:  lipgloss.Color(""),
		symbolFg:  lipgloss.Color(""),
		codeBg:    lipgloss.Color(""),
		codeFg:    theme.Foreground(),
	}

	// Missing line style (for empty side - light grey background)
	missingStyle := lineStyle{
		lineNumBg: lipgloss.Color("#e0e0e0"), // light grey
		lineNumFg: lipgloss.Color("#9e9e9e"), // medium grey
		symbolBg:  lipgloss.Color("#e0e0e0"),
		symbolFg:  lipgloss.Color("#9e9e9e"),
		codeBg:    lipgloss.Color("#e0e0e0"),
		codeFg:    lipgloss.Color("#9e9e9e"),
	}

	hunkStyle := lipgloss.NewStyle().Foreground(theme.Color(ThemeColorModelPickerMutedForeground)).Bold(true)

	lexer := getLexer(filePath)
	chromaStyle := getChromaStyle()

	var result []string

	for _, sh := range splitHunks {
		// Render hunk header
		result = append(result, hunkStyle.Render(shrinkStringWidth(sh.header, width)))

		beforeLine := sh.fromLine
		afterLine := sh.toLine

		for _, sl := range sh.lines {
			// Determine styles for each side
			leftStyle := missingStyle
			rightStyle := missingStyle

			if sl.hasBefore {
				switch sl.before.Kind {
				case udiff.Equal:
					leftStyle = equalStyle
				case udiff.Delete:
					leftStyle = deleteStyle
				}
			}

			if sl.hasAfter {
				switch sl.after.Kind {
				case udiff.Equal:
					rightStyle = equalStyle
				case udiff.Insert:
					rightStyle = insertStyle
				}
			}

			// Build left and right cells (may be multiple lines due to wrapping)
			leftCells := buildCell(sl.hasBefore, sl.before, beforeLine, numWidth, columnWidth, leftStyle, lexer, chromaStyle)
			if sl.hasBefore && sl.before.Kind != udiff.Insert {
				beforeLine++
			}

			rightCells := buildCell(sl.hasAfter, sl.after, afterLine, numWidth, columnWidth, rightStyle, lexer, chromaStyle)
			if sl.hasAfter && sl.after.Kind != udiff.Delete {
				afterLine++
			}

			// Pad shorter side to match
			maxLines := max(len(leftCells), len(rightCells))
			for len(leftCells) < maxLines {
				emptyStyle := lipgloss.NewStyle().Background(leftStyle.codeBg)
				leftCells = append(leftCells, emptyStyle.Render(strings.Repeat(" ", columnWidth)))
			}
			for len(rightCells) < maxLines {
				emptyStyle := lipgloss.NewStyle().Background(rightStyle.codeBg)
				rightCells = append(rightCells, emptyStyle.Render(strings.Repeat(" ", columnWidth)))
			}

			// Combine cells with separator
			for i := 0; i < maxLines; i++ {
				sep := buildSeparator(leftStyle.codeBg, rightStyle.codeBg)
				result = append(result, leftCells[i]+sep+rightCells[i])
			}
		}
	}

	return result
}

// RenderUnifiedDiff renders a unified diff view.
func RenderUnifiedDiff(oldContent, newContent, filePath string, width int, theme Theme) []string {
	if oldContent == newContent {
		return []string{"(no changes)"}
	}

	edits := udiff.Lines(oldContent, newContent)
	unified, err := udiff.ToUnifiedDiff(filePath, filePath, oldContent, edits, 3)
	if err != nil || len(unified.Hunks) == 0 {
		return []string{"(no diff)"}
	}

	// Find max line number for padding
	maxLine := 0
	for _, h := range unified.Hunks {
		for _, l := range h.Lines {
			_ = l // Just count from hunk ranges
		}
		maxLine = max(maxLine, h.FromLine+len(h.Lines))
		maxLine = max(maxLine, h.ToLine+len(h.Lines))
	}
	numWidth := max(2, len(strconv.Itoa(maxLine)))

	// Styles
	fg := lipgloss.Color("#1a1a1a")

	deleteStyle := lineStyle{
		lineNumBg: lipgloss.Color("#ffebee"),
		lineNumFg: lipgloss.Color("#c62828"),
		symbolBg:  lipgloss.Color("#ffebee"),
		symbolFg:  lipgloss.Color("#c62828"),
		codeBg:    lipgloss.Color("#ffebee"),
		codeFg:    fg,
	}

	insertStyle := lineStyle{
		lineNumBg: lipgloss.Color("#e8f5e9"),
		lineNumFg: lipgloss.Color("#2e7d32"),
		symbolBg:  lipgloss.Color("#e8f5e9"),
		symbolFg:  lipgloss.Color("#2e7d32"),
		codeBg:    lipgloss.Color("#e8f5e9"),
		codeFg:    fg,
	}

	equalStyle := lineStyle{
		lineNumBg: lipgloss.Color(""),
		lineNumFg: lipgloss.Color("8"),
		symbolBg:  lipgloss.Color(""),
		symbolFg:  lipgloss.Color(""),
		codeBg:    lipgloss.Color(""),
		codeFg:    theme.Foreground(),
	}

	hunkStyle := lipgloss.NewStyle().Foreground(theme.Color(ThemeColorModelPickerMutedForeground)).Bold(true)

	lexer := getLexer(filePath)
	chromaStyle := getChromaStyle()

	var result []string

	for _, h := range unified.Hunks {
		// Render hunk header
		header := formatHunkHeader(h)
		result = append(result, hunkStyle.Render(shrinkStringWidth(header, width)))

		oldLine := h.FromLine
		newLine := h.ToLine

		for _, l := range h.Lines {
			var ls lineStyle
			switch l.Kind {
			case udiff.Delete:
				ls = deleteStyle
			case udiff.Insert:
				ls = insertStyle
			default:
				ls = equalStyle
			}

			// Build unified line
			lines := buildUnifiedLine(l, oldLine, newLine, numWidth, width, ls, lexer, chromaStyle)
			result = append(result, lines...)

			// Update line numbers
			switch l.Kind {
			case udiff.Delete:
				oldLine++
			case udiff.Insert:
				newLine++
			default:
				oldLine++
				newLine++
			}
		}
	}

	return result
}

func buildUnifiedLine(line udiff.Line, oldLineNum, newLineNum, numWidth, width int, ls lineStyle, lexer chroma.Lexer, chromaStyle *chroma.Style) []string {
	content := strings.TrimSuffix(line.Content, "\n")

	// Symbol
	var symbol string
	var symbolStyle lipgloss.Style
	switch line.Kind {
	case udiff.Delete:
		symbol = "-"
		symbolStyle = lipgloss.NewStyle().Background(ls.symbolBg).Foreground(ls.symbolFg).Bold(true)
	case udiff.Insert:
		symbol = "+"
		symbolStyle = lipgloss.NewStyle().Background(ls.symbolBg).Foreground(ls.symbolFg).Bold(true)
	default:
		symbol = " "
		symbolStyle = lipgloss.NewStyle().Background(ls.codeBg)
	}
	renderedSymbol := symbolStyle.Render(symbol)

	// Line numbers (old new format for unified)
	var numStr string
	switch line.Kind {
	case udiff.Delete:
		numStr = fmt.Sprintf("%*d   ", numWidth, oldLineNum)
	case udiff.Insert:
		numStr = fmt.Sprintf("   %*d", numWidth, newLineNum)
	default:
		numStr = fmt.Sprintf("%*d %*d", numWidth, oldLineNum, numWidth, newLineNum)
	}
	numStyle := lipgloss.NewStyle().Background(ls.lineNumBg).Foreground(ls.lineNumFg)
	renderedNum := numStyle.Render(numStr)

	// Code content with syntax highlighting
	highlighted := highlightCode(content, lexer, chromaStyle, ls.codeBg)
	codeStyle := lipgloss.NewStyle().Background(ls.codeBg)
	renderedCode := codeStyle.Render(highlighted)

	// Space
	spaceStyle := lipgloss.NewStyle().Background(ls.codeBg)
	renderedSpace := spaceStyle.Render(" ")

	// Prefix width (line numbers + space + symbol + space)
	prefix := renderedNum + renderedSpace + renderedSymbol + renderedSpace
	prefixWidth := ansi.StringWidth(prefix)

	// Calculate available width for code
	codeWidth := width - prefixWidth
	if codeWidth < 10 {
		codeWidth = 10
	}

	// Wrap code if needed
	codeLines := wrapString(renderedCode, codeWidth, ls.codeBg)

	var result []string
	for i, codeLine := range codeLines {
		if i == 0 {
			// First line: show prefix
			fullLine := prefix + codeLine
			// Pad to width
			if ansi.StringWidth(fullLine) < width {
				padding := strings.Repeat(" ", width-ansi.StringWidth(fullLine))
				fullLine += spaceStyle.Render(padding)
			}
			result = append(result, fullLine)
		} else {
			// Wrapped lines: show continuation prefix
			contPrefix := numStyle.Render(strings.Repeat(" ", ansi.StringWidth(renderedNum))) +
				spaceStyle.Render(" ") +
				spaceStyle.Render(" ")
			fullLine := contPrefix + codeLine
			if ansi.StringWidth(fullLine) < width {
				padding := strings.Repeat(" ", width-ansi.StringWidth(fullLine))
				fullLine += spaceStyle.Render(padding)
			}
			result = append(result, fullLine)
		}
	}

	return result
}

func buildCell(hasContent bool, line udiff.Line, lineNum, numWidth, cellWidth int, ls lineStyle, lexer chroma.Lexer, chromaStyle *chroma.Style) []string {
	if !hasContent {
		// Empty cell
		style := lipgloss.NewStyle().Background(ls.codeBg).Foreground(ls.codeFg)
		content := strings.Repeat(" ", cellWidth)
		return []string{style.Render(content)}
	}

	content := strings.TrimSuffix(line.Content, "\n")

	// Line number
	numStr := fmt.Sprintf("%*d", numWidth, lineNum)
	numStyle := lipgloss.NewStyle().Background(ls.lineNumBg).Foreground(ls.lineNumFg)
	renderedNum := numStyle.Render(numStr)

	// Symbol
	var symbol string
	var symbolStyle lipgloss.Style
	switch line.Kind {
	case udiff.Delete:
		symbol = "-"
		symbolStyle = lipgloss.NewStyle().Background(ls.symbolBg).Foreground(ls.symbolFg).Bold(true)
	case udiff.Insert:
		symbol = "+"
		symbolStyle = lipgloss.NewStyle().Background(ls.symbolBg).Foreground(ls.symbolFg).Bold(true)
	default:
		symbol = " "
		symbolStyle = lipgloss.NewStyle().Background(ls.codeBg)
	}
	renderedSymbol := symbolStyle.Render(symbol)

	// Code content with syntax highlighting
	highlighted := highlightCode(content, lexer, chromaStyle, ls.codeBg)
	codeStyle := lipgloss.NewStyle().Background(ls.codeBg)
	renderedCode := codeStyle.Render(highlighted)

	// Space between symbol and code
	spaceStyle := lipgloss.NewStyle().Background(ls.codeBg)
	renderedSpace := spaceStyle.Render(" ")

	// Prefix width (line number + symbol + space)
	prefix := renderedNum + renderedSymbol + renderedSpace
	prefixWidth := ansi.StringWidth(prefix)

	// Calculate available width for code
	codeWidth := cellWidth - prefixWidth
	if codeWidth < 10 {
		codeWidth = 10
	}

	// Wrap code if needed
	codeLines := wrapString(renderedCode, codeWidth, ls.codeBg)

	var result []string
	for i, codeLine := range codeLines {
		var fullLine string
		if i == 0 {
			// First line: show full prefix
			fullLine = prefix + codeLine
		} else {
			// Wrapped lines: show continuation prefix (empty line number area + empty symbol + space)
			contPrefix := numStyle.Render(strings.Repeat(" ", ansi.StringWidth(renderedNum))) +
				symbolStyle.Render(" ") +
				spaceStyle.Render(" ")
			fullLine = contPrefix + codeLine
		}
		// Pad to cellWidth
		if ansi.StringWidth(fullLine) < cellWidth {
			padding := strings.Repeat(" ", cellWidth-ansi.StringWidth(fullLine))
			fullLine += spaceStyle.Render(padding)
		}
		result = append(result, fullLine)
	}

	return result
}

// buildSeparator creates a separator with background colors from each side
func buildSeparator(leftBg, rightBg lipgloss.Color) string {
	// Left space with left background
	leftSpace := lipgloss.NewStyle().Background(leftBg).Render(" ")
	// Vertical bar (no background)
	bar := "│"
	// Right space with right background
	rightSpace := lipgloss.NewStyle().Background(rightBg).Render(" ")
	return leftSpace + bar + rightSpace
}

func getLexer(filePath string) chroma.Lexer {
	l := lexers.Match(filePath)
	if l == nil {
		l = lexers.Fallback
	}
	return chroma.Coalesce(l)
}

func getChromaStyle() *chroma.Style {
	return chroma.MustNewStyle("hh", chroma.StyleEntries{
		chroma.Text:                "#1a1a1a",
		chroma.Keyword:             "#0000ff",
		chroma.KeywordDeclaration:  "#0000ff",
		chroma.KeywordNamespace:    "#267f99",
		chroma.KeywordType:         "#267f99",
		chroma.String:              "#a31515",
		chroma.Comment:             "#008000",
		chroma.Number:              "#098658",
		chroma.Operator:            "#1a1a1a",
		chroma.Punctuation:         "#1a1a1a",
		chroma.NameFunction:        "#795e26",
		chroma.NameVariable:        "#001080",
		chroma.NameClass:           "#267f99",
		chroma.NameBuiltin:         "#267f99",
		chroma.LiteralStringEscape: "#ee0000",
	})
}

func highlightCode(source string, lexer chroma.Lexer, style *chroma.Style, bgColor lipgloss.Color) string {
	if strings.TrimSpace(source) == "" {
		return source
	}

	it, err := lexer.Tokenise(nil, source)
	if err != nil {
		return source
	}

	var b strings.Builder
	for token := it(); token != chroma.EOF; token = it() {
		entry := style.Get(token.Type)

		s := lipgloss.NewStyle()
		if bgColor != "" {
			s = s.Background(bgColor)
		}

		if !entry.IsZero() {
			if entry.Colour.IsSet() {
				s = s.Foreground(lipgloss.Color(entry.Colour.String()))
			}
			if entry.Bold == chroma.Yes {
				s = s.Bold(true)
			}
		}

		b.WriteString(s.Render(token.Value))
	}

	return b.String()
}

func limitStringWidth(s string, width int) string {
	if ansi.StringWidth(s) <= width {
		return s
	}
	return ansi.Truncate(s, width, "…")
}

// shrinkStringWidth truncates a string to fit within width
func shrinkStringWidth(s string, width int) string {
	if ansi.StringWidth(s) <= width {
		return s
	}
	return ansi.Truncate(s, width, "…")
}

// wrapString wraps a string to fit within the given width, returning multiple lines
func wrapString(s string, width int, bg lipgloss.Color) []string {
	if width <= 0 {
		return []string{s}
	}

	totalWidth := ansi.StringWidth(s)
	if totalWidth <= width {
		return []string{s}
	}

	var lines []string
	remaining := s

	for len(remaining) > 0 {
		remWidth := ansi.StringWidth(remaining)
		if remWidth <= width {
			lines = append(lines, remaining)
			break
		}

		// Find truncation point - we need to handle ANSI codes carefully
		truncated := ansi.Truncate(remaining, width, "")
		lines = append(lines, truncated)

		// Calculate how many visible chars we took
		tookWidth := ansi.StringWidth(truncated)

		// Skip past the taken characters in the original string
		// We need to strip ANSI codes to count properly
		remaining = skipVisibleChars(remaining, tookWidth)
	}

	if len(lines) == 0 {
		return []string{s}
	}

	return lines
}

// skipVisibleChars skips n visible characters in a string with ANSI codes
func skipVisibleChars(s string, n int) string {
	// Use ansi.Truncate to get first n chars, then strip them
	first := ansi.Truncate(s, n, "")
	if len(first) >= len(s) {
		return ""
	}

	// Strip ANSI sequences from first part to count raw bytes
	// We need to find where the nth visible character ends
	inEscape := false
	visibleCount := 0
	bytePos := 0

	for i := 0; i < len(s); i++ {
		c := s[i]
		if c == '\x1b' {
			inEscape = true
			continue
		}
		if inEscape {
			if c == 'm' || c == 'K' || c == 'J' || c == 'H' || c == 'f' {
				inEscape = false
			}
			continue
		}
		visibleCount++
		bytePos = i + 1
		if visibleCount > n {
			// Check if this is a multi-byte UTF-8 character
			for bytePos < len(s) && s[bytePos]&0xC0 == 0x80 {
				bytePos++
			}
			return s[bytePos:]
		}
	}
	return ""
}
