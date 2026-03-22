package tui

import (
	"bytes"
	"os"
	"os/exec"
	"path/filepath"
	"sort"
	"strconv"
	"strings"
	"time"

	"github.com/liznear/hh/tui/session"
)

const sidebarGitRefreshInterval = 2 * time.Second

type modifiedFileStat struct {
	Path    string
	Added   int
	Deleted int
}

func defaultContextWindowTokens(modelName string) int {
	if v := strings.TrimSpace(os.Getenv("HH_CONTEXT_WINDOW")); v != "" {
		if parsed, err := strconv.Atoi(v); err == nil && parsed > 0 {
			return parsed
		}
	}

	name := strings.ToLower(strings.TrimSpace(modelName))
	switch {
	case strings.Contains(name, "gpt-4.1"):
		return 1048576
	case strings.Contains(name, "claude"):
		return 200000
	case strings.Contains(name, "qwen"):
		return 131072
	case strings.Contains(name, "glm"):
		return 128000
	default:
		return 128000
	}
}

func estimateSessionTokenUsage(s *session.State) int {
	if s == nil {
		return 0
	}
	used := 0
	for _, turn := range s.Turns {
		if turn == nil {
			continue
		}
		for _, item := range turn.Items {
			switch typed := item.(type) {
			case *session.UserMessage:
				used += estimateTextTokens(typed.Content) + 4
			case *session.AssistantMessage:
				used += estimateTextTokens(typed.Content) + 4
			case *session.ThinkingBlock:
				used += estimateTextTokens(typed.Content) + 2
			case *session.ToolCallItem:
				used += estimateTextTokens(typed.Name) + estimateTextTokens(typed.Arguments) + 6
				if typed.Result != nil {
					used += estimateTextTokens(typed.Result.Data) + 2
				}
			case *session.ErrorItem:
				used += estimateTextTokens(typed.Message) + 2
			}
		}
	}
	return max(0, used)
}

func estimateTextTokens(s string) int {
	s = strings.TrimSpace(s)
	if s == "" {
		return 0
	}
	runes := len([]rune(s))
	return max(1, (runes+3)/4)
}

func detectWorkingDirectory() string {
	wd, err := os.Getwd()
	if err != nil || strings.TrimSpace(wd) == "" {
		return "."
	}
	return wd
}

func detectGitBranch(workingDir string) string {
	if strings.TrimSpace(workingDir) == "" {
		return ""
	}
	out, err := exec.Command("git", "-C", workingDir, "symbolic-ref", "--quiet", "--short", "HEAD").Output()
	if err != nil {
		return ""
	}
	branch := strings.TrimSpace(string(out))
	if branch == "" || branch == "HEAD" {
		return ""
	}
	return branch
}

func collectModifiedFiles(workingDir string) []modifiedFileStat {
	if strings.TrimSpace(workingDir) == "" {
		return nil
	}

	filesByPath := map[string]modifiedFileStat{}

	diffOut, err := exec.Command("git", "-C", workingDir, "diff", "--numstat", "HEAD", "--").Output()
	if err != nil {
		return nil
	}
	for _, line := range strings.Split(string(diffOut), "\n") {
		line = strings.TrimSpace(line)
		if line == "" {
			continue
		}
		parts := strings.Split(line, "\t")
		if len(parts) < 3 {
			continue
		}
		added, addErr := strconv.Atoi(parts[0])
		deleted, delErr := strconv.Atoi(parts[1])
		if addErr != nil {
			added = 0
		}
		if delErr != nil {
			deleted = 0
		}
		path := strings.TrimSpace(parts[2])
		if path == "" {
			continue
		}
		filesByPath[path] = modifiedFileStat{Path: path, Added: added, Deleted: deleted}
	}

	untrackedOut, err := exec.Command("git", "-C", workingDir, "ls-files", "--others", "--exclude-standard").Output()
	if err == nil {
		for _, line := range strings.Split(string(untrackedOut), "\n") {
			path := strings.TrimSpace(line)
			if path == "" {
				continue
			}
			if _, exists := filesByPath[path]; exists {
				continue
			}
			added := lineCount(filepath.Join(workingDir, path))
			filesByPath[path] = modifiedFileStat{Path: path, Added: added}
		}
	}

	files := make([]modifiedFileStat, 0, len(filesByPath))
	for _, file := range filesByPath {
		files = append(files, file)
	}
	sort.Slice(files, func(i, j int) bool {
		return files[i].Path < files[j].Path
	})
	return files
}

func displayPath(path string) string {
	path = strings.TrimSpace(path)
	if path == "" {
		return path
	}
	return filepath.Clean(path)
}

func lineCount(path string) int {
	data, err := os.ReadFile(path)
	if err != nil || len(data) == 0 {
		return 0
	}
	count := bytes.Count(data, []byte("\n"))
	if data[len(data)-1] != '\n' {
		count++
	}
	return max(0, count)
}
