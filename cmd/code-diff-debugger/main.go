package main

import (
	"fmt"
	"strings"

	tea "charm.land/bubbletea/v2"
	"github.com/charmbracelet/lipgloss"
	"github.com/liznear/hh/tui"
)

type model struct {
	width       int
	height      int
	scrollY     int
	diffLines   []string
	maxScroll   int
	theme       tui.Theme
	viewMode    tui.DiffViewMode
	oldContent  string
	newContent  string
	filePath    string
	cachedWidth int
}

func main() {
	// Sample old and new content for testing
	oldContent := `package main

import "fmt"

func oldFunction() {
	fmt.Println("old code here")
	// This is a comment that will be removed
	x := 1
	y := 2
	return x + y
}

func unchangedFunction() {
	fmt.Println("this stays the same")
}

type OldStruct struct {
	Name string
	Age  int
}

func (s *OldStruct) MethodToRemove() {
	fmt.Println("removing this")
}

const (
	OldConstant = "old value"
)
`

	newContent := `package main

import (
	"fmt"
	"strings"
)

func newFunction() {
	fmt.Println("new code here")
	// This is a new comment
	x := 10
	y := 20
	z := 30
	return x + y + z
}

func unchangedFunction() {
	fmt.Println("this stays the same")
}

type NewStruct struct {
	Name    string
	Age     int
	Address string
}

func (s *NewStruct) NewMethod() {
	fmt.Println("new method")
}

const (
	NewConstant = "new value"
)
`

	theme := tui.DefaultTheme()

	p := tea.NewProgram(&model{
		theme:      theme,
		viewMode:   tui.DiffViewUnified,
		oldContent: oldContent,
		newContent: newContent,
		filePath:   "sample.go",
	})
	if _, err := p.Run(); err != nil {
		fmt.Printf("Error: %v\n", err)
	}
}

func (m *model) Init() tea.Cmd {
	return nil
}

func (m *model) Update(msg tea.Msg) (tea.Model, tea.Cmd) {
	switch msg := msg.(type) {
	case tea.WindowSizeMsg:
		m.width = msg.Width
		m.height = msg.Height
		m.updateDiff()
		m.updateMaxScroll()

	case tea.KeyPressMsg:
		switch msg.String() {
		case "q", "ctrl+c":
			return m, tea.Quit
		case "j", "down":
			if m.scrollY < m.maxScroll {
				m.scrollY++
			}
		case "k", "up":
			if m.scrollY > 0 {
				m.scrollY--
			}
		case "J":
			m.scrollY = min(m.scrollY+5, m.maxScroll)
		case "K":
			m.scrollY = max(m.scrollY-5, 0)
		case "g":
			m.scrollY = 0
		case "G":
			m.scrollY = m.maxScroll
		case "tab":
			// Toggle view mode
			if m.viewMode == tui.DiffViewUnified {
				m.viewMode = tui.DiffViewSplit
			} else {
				m.viewMode = tui.DiffViewUnified
			}
			m.scrollY = 0
			m.updateDiff()
			m.updateMaxScroll()
		}
	}

	return m, nil
}

func (m *model) updateDiff() {
	if m.width == 0 {
		return
	}
	// Auto-switch to unified if width < 100
	effectiveMode := m.viewMode
	if m.width < 100 {
		effectiveMode = tui.DiffViewUnified
	}
	if effectiveMode == tui.DiffViewUnified {
		m.diffLines = tui.RenderUnifiedDiff(m.oldContent, m.newContent, m.filePath, m.width, m.theme)
	} else {
		m.diffLines = tui.RenderSplitDiff(m.oldContent, m.newContent, m.filePath, m.width, m.theme)
	}
	m.cachedWidth = m.width
}

func (m *model) updateMaxScroll() {
	visibleLines := m.height - 5 // Reserve space for header and help
	if visibleLines < 1 {
		visibleLines = 1
	}
	m.maxScroll = max(0, len(m.diffLines)-visibleLines)
}

func (m *model) View() tea.View {
	if m.width == 0 {
		return tea.NewView("Loading...")
	}

	visibleHeight := m.height - 5
	if visibleHeight < 1 {
		visibleHeight = 1
	}

	var b strings.Builder
	b.Grow(visibleHeight * 150)

	// Header
	headerStyle := lipgloss.NewStyle().Bold(true).Foreground(lipgloss.Color("12"))
	b.WriteString(headerStyle.Render("Code Diff Debugger"))
	b.WriteByte('\n')

	// Help line
	helpStyle := lipgloss.NewStyle().Foreground(lipgloss.Color("8"))
	modeStr := "unified"
	if m.viewMode == tui.DiffViewSplit && m.width >= 100 {
		modeStr = "split"
	}
	b.WriteString(helpStyle.Render(fmt.Sprintf("j/k: scroll ↓↑ | J/K: fast scroll | g/G: top/bottom | Tab: switch view (%s) | q: quit", modeStr)))
	b.WriteString("\n\n")

	// Calculate visible lines
	start := m.scrollY
	end := min(len(m.diffLines), start+visibleHeight)

	// Write visible diff lines
	for i := start; i < end; i++ {
		b.WriteString(m.diffLines[i])
		b.WriteByte('\n')
	}

	// Scroll indicator
	if len(m.diffLines) > visibleHeight {
		b.WriteByte('\n')
		b.WriteString(helpStyle.Render(fmt.Sprintf("Lines %d-%d / %d", start+1, end, len(m.diffLines))))
	}

	return tea.NewView(b.String())
}
