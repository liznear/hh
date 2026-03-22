package tui

import (
	"fmt"

	"github.com/liznear/hh/agent"
	"github.com/liznear/hh/tools"
)

type Agent struct {
	Name         string
	SystemPrompt string
	AllowedTools []string
}

var agents = map[string]Agent{
	"Build": {
		Name:         "Build",
		SystemPrompt: "You are Build, a software engineering agent focused on making correct, maintainable code changes.",
		AllowedTools: nil,
	},
}

func newAgentRunner(modelName string, provider agent.Provider, agentName string) (*agent.AgentRunner, error) {
	agentConfig, err := getAgent(agentName)
	if err != nil {
		return nil, err
	}

	return agent.NewAgentRunner(
		modelName,
		provider,
		agent.WithSystemPrompt(agentConfig.SystemPrompt),
		agent.WithTools(resolveTools(agentConfig)),
	), nil
}

func getAgent(name string) (Agent, error) {
	if name == "" {
		name = "Build"
	}
	agentConfig, ok := agents[name]
	if !ok {
		return Agent{}, fmt.Errorf("agent %q not found", name)
	}
	return agentConfig, nil
}

func resolveTools(agentConfig Agent) map[string]agent.Tool {
	if agentConfig.AllowedTools == nil {
		return tools.AllTools()
	}
	return tools.GetTools(agentConfig.AllowedTools)
}
