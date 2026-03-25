package tools

import (
	"bufio"
	"context"
	"fmt"
	"os"
	"strings"

	"github.com/liznear/hh/agent"
)

type ReadResult struct {
	Path      string
	Start     int
	Limit     int
	LineCount int
}

func (r ReadResult) Summary() string {
	return fmt.Sprintf("%d lines", r.LineCount)
}

func NewReadTool() agent.Tool {
	return agent.Tool{
		Name:        "read",
		Description: "Read a range of lines from a file",
		Schema: map[string]any{
			"type": "object",
			"properties": map[string]any{
				"path":  map[string]any{"type": "string"},
				"start": map[string]any{"type": "integer", "minimum": 0},
				"limit": map[string]any{"type": "integer", "minimum": 0},
			},
			"required": []string{"path"},
		},
		Handler: agent.FuncToolHandler(handleRead),
	}
}

func handleRead(_ context.Context, params map[string]any) agent.ToolResult {
	path, err := requiredString(params, "path")
	if err != nil {
		return toolErr("%s", err.Error())
	}
	start, err := optionalInt(params, "start", 0)
	if err != nil {
		return toolErr("%s", err.Error())
	}
	limit, err := optionalInt(params, "limit", 0)
	if err != nil {
		return toolErr("%s", err.Error())
	}

	if start < 0 {
		return toolErr("start must be >= 0")
	}
	if limit < 0 {
		return toolErr("limit must be >= 0")
	}

	file, err := os.Open(path)
	if err != nil {
		return toolErr("failed to open file: %v", err)
	}
	defer file.Close()

	scanner := bufio.NewScanner(file)
	buf := make([]byte, 64*1024)
	scanner.Buffer(buf, 10*1024*1024)

	// limit == 0 means read all lines from start
	readAll := limit == 0
	end := start + limit

	line := 0
	var lines []string
	for scanner.Scan() {
		if line >= start && (readAll || line < end) {
			lines = append(lines, scanner.Text())
		}
		line++
		if !readAll && line >= end {
			break
		}
	}
	if err := scanner.Err(); err != nil {
		return toolErr("failed to read file: %v", err)
	}

	return agent.ToolResult{
		Data: strings.Join(lines, "\n"),
		Result: ReadResult{
			Path:      path,
			Start:     start,
			Limit:     limit,
			LineCount: len(lines),
		},
	}
}

func toolErr(format string, args ...any) agent.ToolResult {
	return agent.ToolResult{IsErr: true, Data: fmt.Sprintf(format, args...)}
}
