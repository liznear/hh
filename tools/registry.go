package tools

import (
	"github.com/liznear/hh/agent"
	"golang.org/x/exp/maps"
)

var toolsCreator = map[string]func() agent.Tool{
	"read":       NewReadTool,
	"edit":       NewEditTool,
	"grep":       NewGrepTool,
	"list":       NewListTool,
	"glob":       NewGlobTool,
	"todo_write": NewTodoWriteTool,
	"web_fetch":  NewWebFetchTool,
	"web_search": NewWebSearchTool,
}

func AllTools() map[string]agent.Tool {
	return GetTools(maps.Keys(toolsCreator))
}

func GetTools(tools []string) map[string]agent.Tool {
	ret := make(map[string]agent.Tool, len(tools))
	for _, tool := range tools {
		ret[tool] = toolsCreator[tool]()
	}
	return ret
}
