package tools

import (
	"bufio"
	"context"
	"fmt"
	"os"
	"strings"

	"github.com/liznear/hh/agent"
)

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
			"required": []string{"path", "start", "limit"},
		},
		Handler: agent.FuncToolHandler(handleRead),
	}
}

func handleRead(_ context.Context, params map[string]any) agent.ToolResult {
	path, err := requiredString(params, "path")
	if err != nil {
		return toolErr("%s", err.Error())
	}
	start, err := requiredInt(params, "start")
	if err != nil {
		return toolErr("%s", err.Error())
	}
	limit, err := requiredInt(params, "limit")
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

	end := start + limit
	if limit == 0 {
		end = start
	}

	line := 0
	lines := make([]string, 0, limit)
	for scanner.Scan() {
		if line >= start && line < end {
			lines = append(lines, scanner.Text())
		}
		line++
		if line >= end {
			break
		}
	}
	if err := scanner.Err(); err != nil {
		return toolErr("failed to read file: %v", err)
	}

	return agent.ToolResult{Data: strings.Join(lines, "\n")}
}

func toolErr(format string, args ...any) agent.ToolResult {
	return agent.ToolResult{IsErr: true, Data: fmt.Sprintf(format, args...)}
}
