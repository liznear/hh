package tools

import (
	"context"
	"os"
	"path/filepath"
	"strings"

	"github.com/liznear/hh/agent"
	ignore "github.com/sabhiram/go-gitignore"
)

func NewListTool() agent.Tool {
	return agent.Tool{
		Name:        "list",
		Description: "List files and directories in a path",
		Schema: map[string]any{
			"type": "object",
			"properties": map[string]any{
				"path":            map[string]any{"type": "string"},
				"recursive":       map[string]any{"type": "boolean"},
				"include_ignored": map[string]any{"type": "boolean"},
			},
			"required": []string{"path"},
		},
		Handler: agent.FuncToolHandler(handleList),
	}
}

func handleList(_ context.Context, params map[string]any) agent.ToolResult {
	path, err := requiredString(params, "path")
	if err != nil {
		return toolErr("%s", err.Error())
	}

	recursive, err := optionalBool(params, "recursive")
	if err != nil {
		return toolErr("%s", err.Error())
	}

	includeIgnored, err := optionalBool(params, "include_ignored")
	if err != nil {
		return toolErr("%s", err.Error())
	}

	info, err := os.Stat(path)
	if err != nil {
		return toolErr("failed to access path: %v", err)
	}

	if !info.IsDir() {
		return toolErr("path is not a directory: %s", path)
	}

	var gitIgnore *ignore.GitIgnore
	if !includeIgnored {
		gitIgnore = loadGitIgnore(path)
	}

	var entries []string

	if recursive {
		entries, err = listRecursive(path, gitIgnore)
	} else {
		entries, err = listFlat(path, gitIgnore)
	}

	if err != nil {
		return toolErr("failed to list directory: %v", err)
	}

	return agent.ToolResult{Data: strings.Join(entries, "\n")}
}

func listFlat(path string, gitIgnore *ignore.GitIgnore) ([]string, error) {
	dir, err := os.ReadDir(path)
	if err != nil {
		return nil, err
	}

	var entries []string
	for _, entry := range dir {
		name := entry.Name()
		if gitIgnore != nil && gitIgnore.MatchesPath(name) {
			continue
		}

		if entry.IsDir() {
			entries = append(entries, name+"/")
		} else {
			entries = append(entries, name)
		}
	}

	return entries, nil
}

func listRecursive(root string, gitIgnore *ignore.GitIgnore) ([]string, error) {
	var entries []string

	err := filepath.WalkDir(root, func(path string, d os.DirEntry, err error) error {
		if err != nil {
			return err
		}

		if path == root {
			return nil
		}

		relPath, err := filepath.Rel(root, path)
		if err != nil {
			return err
		}

		if gitIgnore != nil && gitIgnore.MatchesPath(relPath) {
			if d.IsDir() {
				return filepath.SkipDir
			}
			return nil
		}

		if d.IsDir() {
			entries = append(entries, relPath+"/")
		} else {
			entries = append(entries, relPath)
		}

		return nil
	})

	return entries, err
}

func loadGitIgnore(path string) *ignore.GitIgnore {
	absPath, err := filepath.Abs(path)
	if err != nil {
		return nil
	}

	ignores := []string{}

	for {
		gitignorePath := filepath.Join(absPath, ".gitignore")
		if data, err := os.ReadFile(gitignorePath); err == nil {
			ignores = append(ignores, strings.Split(string(data), "\n")...)
		}

		parent := filepath.Dir(absPath)
		if parent == absPath {
			break
		}
		absPath = parent
	}

	if len(ignores) == 0 {
		return nil
	}

	gitIgnore := ignore.CompileIgnoreLines(ignores...)

	return gitIgnore
}
