package main

import (
	"fmt"
	"strings"

	tea "charm.land/bubbletea/v2"
	"github.com/charmbracelet/lipgloss"
	"github.com/liznear/hh/tui"
)

type model struct {
	width     int
	height    int
	scrollY   int
	diffLines []string
	maxScroll int
	theme     tui.Theme
	// Pre-computed parts
	header    string
	help      string
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
	diffLines := tui.RenderSplitDiff(oldContent, newContent, "sample.go", 120, theme)

	// Pre-compute static parts
	headerStyle := lipgloss.NewStyle().Bold(true).Foreground(lipgloss.Color("12"))
	helpStyle := lipgloss.NewStyle().Foreground(lipgloss.Color("8"))

	p := tea.NewProgram(&model{
		diffLines: diffLines,
		theme:     theme,
		header:    headerStyle.Render("Code Diff Debugger") + "\n",
		help:      helpStyle.Render("j/k: scroll ↓↑ | J/K: fast scroll | g/G: top/bottom | q: quit") + "\n\n",
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
		}
	}

	return m, nil
}

func (m *model) updateMaxScroll() {
	visibleLines := m.height - 4 // Reserve space for header and help
	if visibleLines < 1 {
		visibleLines = 1
	}
	m.maxScroll = max(0, len(m.diffLines)-visibleLines)
}

func (m *model) View() tea.View {
	if m.width == 0 {
		return tea.NewView("Loading...")
	}

	// Pre-allocate builder with estimated size
	visibleHeight := m.height - 4
	if visibleHeight < 1 {
		visibleHeight = 1
	}

	var b strings.Builder
	b.Grow(visibleHeight * 150) // Estimate ~150 chars per line

	// Header
	b.WriteString(m.header)

	// Help line
	b.WriteString(m.help)

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
		helpStyle := lipgloss.NewStyle().Foreground(lipgloss.Color("8"))
		fmt.Fprintf(&b, "\n%s", helpStyle.Render(fmt.Sprintf("Lines %d-%d / %d", start+1, end, len(m.diffLines))))
	}

	return tea.NewView(b.String())
}
