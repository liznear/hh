package tools

import (
	"context"
	"fmt"
	"os"
	"path/filepath"
	"sort"
	"strings"

	"github.com/bmatcuk/doublestar/v4"
	"github.com/liznear/hh/agent"
)

type GlobResult struct {
	Pattern    string
	Path       string
	MatchCount int
}

func (r GlobResult) Summary() string {
	if r.MatchCount == 0 {
		return "no matches"
	}
	return fmt.Sprintf("%d matches", r.MatchCount)
}

func NewGlobTool() agent.Tool {
	return agent.Tool{
		Name:        "glob",
		Description: "Find files matching a glob pattern in a directory",
		Schema: map[string]any{
			"type": "object",
			"properties": map[string]any{
				"pattern": map[string]any{"type": "string", "description": "Glob pattern to match files (e.g., \"*.go\", \"**/*.txt\")"},
				"path":    map[string]any{"type": "string", "description": "Directory to search in (defaults to current directory)"},
			},
			"required": []string{"pattern"},
		},
		Handler: agent.FuncToolHandler(handleGlob),
	}
}

func handleGlob(_ context.Context, params map[string]any) agent.ToolResult {
	pattern, err := requiredString(params, "pattern")
	if err != nil {
		return toolErr("%s", err.Error())
	}

	path, err := optionalString(params, "path")
	if err != nil {
		return toolErr("%s", err.Error())
	}
	if path == "" {
		path = "."
	}

	info, err := os.Stat(path)
	if err != nil {
		return toolErr("failed to access path: %v", err)
	}

	if !info.IsDir() {
		return toolErr("path is not a directory: %s", path)
	}

	matches, err := doublestar.FilepathGlob(filepath.Join(path, pattern))
	if err != nil {
		return toolErr("invalid glob pattern: %v", err)
	}

	// Make paths relative to the search directory
	relMatches := make([]string, 0, len(matches))
	for _, match := range matches {
		relPath, err := filepath.Rel(path, match)
		if err != nil {
			relPath = match
		}
		relMatches = append(relMatches, relPath)
	}
	sort.Strings(relMatches)

	return agent.ToolResult{
		Data: strings.Join(relMatches, "\n"),
		Result: GlobResult{
			Pattern:    pattern,
			Path:       path,
			MatchCount: len(relMatches),
		},
	}
}
