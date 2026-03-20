package tools

import "github.com/liznear/hh/agent"

func AllTools() map[string]agent.Tool {
	ret := map[string]agent.Tool{}
	for _, tool := range []agent.Tool{
		NewReadTool(),
		NewEditTool(),
		NewGrepTool(),
	} {
		ret[tool.Name] = tool
	}
	return ret
}
