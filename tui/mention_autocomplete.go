package tui

import (
	"os"
	"os/exec"
	"path"
	"path/filepath"
	"sort"
	"strings"
	"unicode"
	"unicode/utf8"

	tea "charm.land/bubbletea/v2"
	"github.com/bmatcuk/doublestar/v4"
	"github.com/charmbracelet/lipgloss"
	"github.com/liznear/hh/tui/agents"
)

const mentionAutocompleteMaxItems = 5

type mentionSuggestion struct {
	Value   string
	IsAgent bool
	Ignored bool
}

type mentionPath struct {
	Value   string
	Ignored bool
}

type mentionedFileContent struct {
	Path    string
	Content string
}

var listSubAgents = func() ([]string, error) {
	catalog, err := agents.LoadDefaultCatalog()
	if err != nil {
		return nil, err
	}
	subAgents := catalog.SubAgents()
	names := make([]string, 0, len(subAgents))
	for _, subAgent := range subAgents {
		names = append(names, strings.ToLower(strings.TrimSpace(subAgent.Name)))
	}
	sort.Strings(names)
	return names, nil
}

// globMentionPaths returns file/directory paths matching the query. It uses
// git ls-files in git repos (fast, respects .gitignore) and falls back to
// filesystem glob (skipping dot-prefixed entries) otherwise.
var globMentionPaths = globMentionPathsImpl

func globMentionPathsImpl(workingDir string, query string) ([]mentionPath, error) {
	root := strings.TrimSpace(workingDir)
	if root == "" {
		return nil, nil
	}

	// Try git ls-files first — uses the index so it's fast and automatically
	// respects .gitignore. Falls back to filesystem glob for non-git dirs.
	entries, err := gitLsFiles(root)
	if err != nil {
		entries, err = globFiles(root)
		if err != nil {
			return nil, err
		}
	}

	// Filter by query.
	lowerQuery := strings.ToLower(query)
	filtered := make([]mentionPath, 0, len(entries))
	for _, entry := range entries {
		if !pathMatchesQuery(entry, lowerQuery) {
			continue
		}
		filtered = append(filtered, mentionPath{Value: entry})
	}
	return filtered, nil
}

// gitLsFiles returns all non-ignored files and derived directory entries using
// git's index. Returns an error if the directory is not a git repo.
func gitLsFiles(root string) ([]string, error) {
	cmd := exec.Command("git", "-C", root, "ls-files", "--cached", "--others", "--exclude-standard")
	out, err := cmd.Output()
	if err != nil {
		return nil, err
	}

	lines := strings.Split(strings.TrimSpace(string(out)), "\n")
	seen := map[string]struct{}{}
	entries := make([]string, 0, len(lines)*2)

	for _, line := range lines {
		line = strings.TrimSpace(line)
		if line == "" {
			continue
		}
		if _, ok := seen[line]; ok {
			continue
		}
		seen[line] = struct{}{}
		entries = append(entries, filepath.ToSlash(line))

		// Derive directory entries from file paths.
		dir := path.Dir(line)
		for dir != "." && dir != "" {
			slashDir := dir + "/"
			if _, ok := seen[slashDir]; !ok {
				seen[slashDir] = struct{}{}
				entries = append(entries, slashDir)
			}
			dir = path.Dir(dir)
		}
	}

	sort.Strings(entries)
	return entries, nil
}

// globFiles is a fallback for non-git directories using filesystem glob.
func globFiles(root string) ([]string, error) {
	fsys := os.DirFS(root)
	opts := []doublestar.GlobOption{doublestar.WithNoHidden(), doublestar.WithCaseInsensitive()}

	seen := map[string]struct{}{}
	var entries []string

	matches, err := doublestar.Glob(fsys, "**", opts...)
	if err != nil {
		return nil, err
	}
	for _, match := range matches {
		if match == "." || match == "" {
			continue
		}
		if _, ok := seen[match]; ok {
			continue
		}
		seen[match] = struct{}{}

		fullPath := filepath.Join(root, filepath.FromSlash(match))
		info, err := os.Stat(fullPath)
		if err != nil {
			continue
		}
		if info.IsDir() {
			entries = append(entries, match+"/")
		} else {
			entries = append(entries, match)
		}
	}

	sort.Strings(entries)
	return entries, nil
}

func (m *model) updateMentionAutocomplete() {
	active, query, _, _ := parseActiveMention(m.input.Value(), m.input.Line(), m.input.Column())
	if !active {
		m.mentionSuggestions = nil
		m.mentionSelectionIndex = 0
		return
	}

	suggestions, err := buildMentionSuggestions(query, m.workingDir)
	if err != nil {
		m.mentionSuggestions = nil
		m.mentionSelectionIndex = 0
		return
	}
	m.mentionSuggestions = suggestions
	if len(m.mentionSuggestions) == 0 {
		m.mentionSelectionIndex = 0
		return
	}
	if m.mentionSelectionIndex < 0 {
		m.mentionSelectionIndex = 0
	}
	if m.mentionSelectionIndex >= len(m.mentionSuggestions) {
		m.mentionSelectionIndex = len(m.mentionSuggestions) - 1
	}
}

func buildMentionSuggestions(query string, workingDir string) ([]mentionSuggestion, error) {
	query = strings.ToLower(strings.TrimSpace(query))
	out := make([]mentionSuggestion, 0, mentionAutocompleteMaxItems)

	subAgents, err := listSubAgents()
	if err != nil {
		return nil, err
	}
	for _, name := range subAgents {
		if !strings.HasPrefix(name, query) {
			continue
		}
		out = append(out, mentionSuggestion{Value: name, IsAgent: true})
		if len(out) == mentionAutocompleteMaxItems {
			return out, nil
		}
	}

	paths, err := globMentionPaths(workingDir, query)
	if err != nil {
		return nil, err
	}
	regular := make([]mentionPath, 0, len(paths))
	ignored := make([]mentionPath, 0, len(paths))
	for _, candidate := range paths {
		if candidate.Ignored {
			ignored = append(ignored, candidate)
		} else {
			regular = append(regular, candidate)
		}
	}

	for _, candidate := range regular {
		out = append(out, mentionSuggestion{Value: candidate.Value, Ignored: false})
		if len(out) == mentionAutocompleteMaxItems {
			return out, nil
		}
	}
	for _, candidate := range ignored {
		out = append(out, mentionSuggestion{Value: candidate.Value, Ignored: true})
		if len(out) == mentionAutocompleteMaxItems {
			break
		}
	}

	return out, nil
}

func (m *model) applyMentionAutocomplete() bool {
	m.updateMentionAutocomplete()
	active, _, start, end := parseActiveMention(m.input.Value(), m.input.Line(), m.input.Column())
	if !active || len(m.mentionSuggestions) == 0 {
		return false
	}
	selectedIdx := m.mentionSelectionIndex
	if selectedIdx < 0 || selectedIdx >= len(m.mentionSuggestions) {
		selectedIdx = 0
	}
	selectedSuggestion := m.mentionSuggestions[selectedIdx]
	runes := []rune(m.input.Value())
	if start < 0 || end < start || end > len(runes) {
		return false
	}

	replacement := []rune("@" + selectedSuggestion.Value)
	updated := append(append(append([]rune{}, runes[:start]...), replacement...), runes[end:]...)
	m.input.SetValue(string(updated))
	newCursor := start + len(replacement)
	setTextareaCursorFromOffset(&m.input, newCursor)
	m.updateMentionAutocomplete()
	m.mentionSelectionIndex = 0
	return true
}

func (m *model) handleMentionSelectionKey(msg tea.KeyPressMsg) bool {
	if len(m.mentionSuggestions) == 0 {
		return false
	}
	active, _, _, _ := parseActiveMention(m.input.Value(), m.input.Line(), m.input.Column())
	if !active {
		return false
	}

	switch msg.Key().Code {
	case tea.KeyUp:
		if m.mentionSelectionIndex > 0 {
			m.mentionSelectionIndex--
		}
		return true
	case tea.KeyDown:
		if m.mentionSelectionIndex < len(m.mentionSuggestions)-1 {
			m.mentionSelectionIndex++
		}
		return true
	default:
		return false
	}
}

func (m *model) mentionAutocompleteHeight() int {
	if m.taskSessionView != nil {
		return 0
	}
	active, _, _, _ := parseActiveMention(m.input.Value(), m.input.Line(), m.input.Column())
	if !active {
		return 0
	}
	return len(m.mentionSuggestions)
}

func (m *model) renderMentionAutocomplete(width int) string {
	if len(m.mentionSuggestions) == 0 {
		return ""
	}
	active, _, _, _ := parseActiveMention(m.input.Value(), m.input.Line(), m.input.Column())
	if !active {
		return ""
	}
	popupWidth := max(1, width)
	muted := lipgloss.NewStyle().Foreground(m.theme.Color(ThemeColorModelPickerMutedForeground))
	selected := lipgloss.NewStyle().Bold(true)
	selectedIdx := m.mentionSelectionIndex
	if selectedIdx < 0 || selectedIdx >= len(m.mentionSuggestions) {
		selectedIdx = 0
	}
	lines := make([]string, 0, len(m.mentionSuggestions))
	for i, suggestion := range m.mentionSuggestions {
		line := "  @" + suggestion.Value
		if i == selectedIdx {
			line = selected.Render("> @" + suggestion.Value)
		} else {
			line = muted.Render(line)
		}
		lines = append(lines, line)
	}
	return lipgloss.NewStyle().Width(popupWidth).Render(strings.Join(lines, "\n"))
}

func pathMatchesQuery(candidate, query string) bool {
	if query == "" {
		return true
	}
	lower := strings.ToLower(candidate)
	if strings.HasPrefix(lower, query) {
		return true
	}
	base := strings.ToLower(path.Base(strings.TrimSuffix(candidate, "/")))
	return strings.HasPrefix(base, query)
}

func parseActiveMention(input string, line int, col int) (active bool, query string, start int, end int) {
	offset := cursorOffset(input, line, col)
	runes := []rune(input)
	if offset < 0 || offset > len(runes) {
		offset = len(runes)
	}

	tokenStart := offset
	for tokenStart > 0 && !unicode.IsSpace(runes[tokenStart-1]) {
		tokenStart--
	}
	if tokenStart >= len(runes) || runes[tokenStart] != '@' {
		return false, "", 0, 0
	}
	if tokenStart > 0 && !unicode.IsSpace(runes[tokenStart-1]) {
		return false, "", 0, 0
	}

	tokenEnd := tokenStart + 1
	for tokenEnd < len(runes) && !unicode.IsSpace(runes[tokenEnd]) {
		tokenEnd++
	}
	if offset > tokenEnd {
		return false, "", 0, 0
	}

	queryRunes := runes[tokenStart+1 : offset]
	return true, strings.ToLower(string(queryRunes)), tokenStart, offset
}

func cursorOffset(input string, line int, col int) int {
	if line < 0 {
		line = 0
	}
	if col < 0 {
		col = 0
	}

	lines := strings.Split(input, "\n")
	if len(lines) == 0 {
		return 0
	}
	if line >= len(lines) {
		line = len(lines) - 1
	}

	offset := 0
	for i := 0; i < line; i++ {
		offset += utf8.RuneCountInString(lines[i]) + 1
	}

	lineLen := utf8.RuneCountInString(lines[line])
	if col > lineLen {
		col = lineLen
	}
	return offset + col
}

func setTextareaCursorFromOffset(in interface {
	SetCursorColumn(col int)
	MoveToBegin()
	CursorDown()
	Line() int
	Column() int
}, offset int) {
	if offset < 0 {
		offset = 0
	}
	in.MoveToBegin()
	for offset > 0 {
		line := in.Line()
		col := in.Column()
		in.SetCursorColumn(col + 1)
		if in.Line() == line && in.Column() == col {
			in.CursorDown()
			if in.Line() == line {
				return
			}
		}
		offset--
	}
}

func (m *model) collectMentionedFileContents(prompt string) []mentionedFileContent {
	mentions := findMentionTokens(prompt)
	if len(mentions) == 0 {
		return nil
	}

	subAgents, _ := listSubAgents()
	subAgentSet := make(map[string]struct{}, len(subAgents))
	for _, name := range subAgents {
		subAgentSet[strings.ToLower(strings.TrimSpace(name))] = struct{}{}
	}

	seen := map[string]struct{}{}
	files := make([]mentionedFileContent, 0, len(mentions))
	for _, mention := range mentions {
		key := strings.ToLower(strings.TrimSpace(mention))
		if key == "" {
			continue
		}
		if _, ok := subAgentSet[key]; ok {
			continue
		}
		if _, ok := seen[key]; ok {
			continue
		}
		seen[key] = struct{}{}

		displayPath, fullPath, ok := resolveMentionFilePath(m.workingDir, mention)
		if !ok {
			continue
		}
		info, err := os.Stat(fullPath)
		if err != nil || info.IsDir() {
			continue
		}
		content, err := os.ReadFile(fullPath)
		if err != nil {
			continue
		}
		files = append(files, mentionedFileContent{Path: displayPath, Content: string(content)})
	}
	return files
}

func findMentionTokens(prompt string) []string {
	runes := []rune(prompt)
	tokens := make([]string, 0, 4)
	for i := 0; i < len(runes); i++ {
		if runes[i] != '@' {
			continue
		}
		if i > 0 && !unicode.IsSpace(runes[i-1]) {
			continue
		}
		j := i + 1
		for j < len(runes) && !unicode.IsSpace(runes[j]) {
			j++
		}
		if j == i+1 {
			continue
		}
		tokens = append(tokens, string(runes[i+1:j]))
		i = j - 1
	}
	return tokens
}

func resolveMentionFilePath(workingDir string, mention string) (displayPath string, fullPath string, ok bool) {
	mention = strings.TrimSpace(mention)
	if mention == "" {
		return "", "", false
	}

	root := strings.TrimSpace(workingDir)
	if root == "" {
		root = "."
	}
	rootAbs, err := filepath.Abs(root)
	if err != nil {
		return "", "", false
	}

	var candidate string
	if filepath.IsAbs(mention) {
		candidate = filepath.Clean(mention)
	} else {
		candidate = filepath.Join(rootAbs, mention)
	}
	candidateAbs, err := filepath.Abs(candidate)
	if err != nil {
		return "", "", false
	}

	rel, err := filepath.Rel(rootAbs, candidateAbs)
	if err != nil {
		return "", "", false
	}
	if rel == ".." || strings.HasPrefix(rel, ".."+string(filepath.Separator)) {
		return "", "", false
	}

	display := filepath.ToSlash(rel)
	if filepath.IsAbs(mention) {
		display = filepath.ToSlash(mention)
	}
	return display, candidateAbs, true
}
