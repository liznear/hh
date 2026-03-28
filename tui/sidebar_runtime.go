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
)

const sidebarGitRefreshInterval = 2 * time.Second

type modifiedFileStat struct {
	Path    string
	Added   int
	Deleted int
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

func gitDiffContentForPath(workingDir string, path string) (oldContent string, newContent string, err error) {
	workingDir = strings.TrimSpace(workingDir)
	path = strings.TrimSpace(path)
	if workingDir == "" || path == "" {
		return "", "", nil
	}

	fullPath := filepath.Join(workingDir, path)

	// Get current file content (new content)
	newBytes, readErr := os.ReadFile(fullPath)
	if readErr != nil {
		return "", "", readErr
	}
	newContent = string(newBytes)

	// Get original content from git (old content)
	out, gitErr := exec.Command("git", "-C", workingDir, "show", "HEAD:"+path).Output()
	if gitErr == nil {
		oldContent = string(out)
		return oldContent, newContent, nil
	}

	// If git show fails (e.g., untracked file), old content is empty
	return "", newContent, nil
}

func displayPath(path string) string {
	return beautifySidebarPath(path, os.Getenv("HOME"))
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
