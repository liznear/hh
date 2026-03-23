package tui

import (
	"fmt"
	"strings"

	"github.com/liznear/hh/agent"
	"github.com/liznear/hh/config"
	"github.com/liznear/hh/skills"
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

func newAgentRunner(modelName string, provider agent.Provider, agentName string, cfg config.Config, workingDir string) (*agent.AgentRunner, error) {
	agentConfig, err := getAgent(agentName)
	if err != nil {
		return nil, err
	}

	approver, err := newToolApprover(cfg, workingDir)
	if err != nil {
		return nil, err
	}

	skillCatalog, err := skills.LoadDefaultCatalog()
	if err != nil {
		return nil, err
	}
	tools.SetSkillCatalog(skillCatalog)
	systemPrompt := buildSystemPrompt(agentConfig.SystemPrompt, skillCatalog)

	return agent.NewAgentRunner(
		modelName,
		provider,
		agent.WithSystemPrompt(systemPrompt),
		agent.WithTools(resolveTools(agentConfig)),
		agent.WithToolApprover(approver),
	), nil
}

func buildSystemPrompt(base string, skillCatalog skills.Catalog) string {
	base = strings.TrimSpace(base)
	if skillCatalog.IsEmpty() {
		return base
	}

	skillBlock := strings.TrimSpace(skillCatalog.PromptFrontmatterBlock())
	if skillBlock == "" {
		return base
	}

	if base == "" {
		return skillBlock
	}

	return base + "\n\n" + skillBlock
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
