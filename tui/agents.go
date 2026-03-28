package tui

import (
	"fmt"
	"os"
	"path/filepath"
	"strings"

	"github.com/liznear/hh/agent"
	"github.com/liznear/hh/config"
	"github.com/liznear/hh/skills"
	"github.com/liznear/hh/tools"
	"github.com/liznear/hh/tui/agents"
)

var listAvailableAgents = func() ([]string, error) {
	catalog, err := agents.LoadDefaultCatalog()
	if err != nil {
		return nil, fmt.Errorf("load agent catalog: %w", err)
	}

	all := catalog.All()
	names := make([]string, 0, len(all))
	for _, agentConfig := range all {
		names = append(names, agentConfig.Name)
	}
	return names, nil
}

var updateRunnerForAgent = func(runner *agent.AgentRunner, agentName string, cfg config.Config, workingDir string) error {
	opts, err := buildAgentOpts(agentName, cfg, workingDir)
	if err != nil {
		return err
	}
	return runner.Update(opts...)
}

func newAgentRunner(modelName string, provider agent.Provider, agentName string, cfg config.Config, workingDir string) (*agent.AgentRunner, error) {
	opts, err := buildAgentOpts(agentName, cfg, workingDir)
	if err != nil {
		return nil, err
	}
	return agent.NewAgentRunner(modelName, provider, opts...), nil
}

func buildAgentOpts(agentName string, cfg config.Config, workingDir string) ([]agent.Opt, error) {
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
	systemPrompt := buildSystemPrompt(agentConfig.SystemPrompt, skillCatalog, workingDir)

	return []agent.Opt{
		agent.WithSystemPrompt(systemPrompt),
		agent.WithTools(resolveTools(agentConfig)),
		agent.WithToolApprover(approver),
	}, nil
}

func buildSystemPrompt(base string, skillCatalog skills.Catalog, workingDir string) string {
	var parts []string

	base = strings.TrimSpace(base)
	if base != "" {
		parts = append(parts, base)
	}

	skillBlock := strings.TrimSpace(skillCatalog.PromptFrontmatterBlock())
	if skillBlock != "" {
		parts = append(parts, skillBlock)
	}

	// Inject global AGENTS.md from ~/.agents/AGENTS.md
	globalAgentsMD := readAgentsMDFile(getGlobalAgentsMDPath())
	if globalAgentsMD != "" {
		parts = append(parts, "<global-agents-md>\n"+globalAgentsMD+"\n</global-agents-md>")
	}

	// Inject project AGENTS.md from working directory
	projectAgentsMD := readAgentsMDFile(projectAgentsMDPath(workingDir))
	if projectAgentsMD != "" {
		parts = append(parts, "<project-agents-md>\n"+projectAgentsMD+"\n</project-agents-md>")
	}

	return strings.Join(parts, "\n\n")
}

// getGlobalAgentsMDPath returns the path to the global AGENTS.md file.
// It can be overridden in tests.
var getGlobalAgentsMDPath = func() string {
	home, err := os.UserHomeDir()
	if err != nil {
		return ""
	}
	return filepath.Join(home, ".agents", "AGENTS.md")
}

func projectAgentsMDPath(workingDir string) string {
	if workingDir == "" {
		return ""
	}
	return filepath.Join(workingDir, "AGENTS.md")
}

func readAgentsMDFile(path string) string {
	if path == "" {
		return ""
	}
	content, err := os.ReadFile(path)
	if err != nil {
		return ""
	}
	return strings.TrimSpace(string(content))
}

func getAgent(name string) (agents.Agent, error) {
	if name == "" {
		name = "Build"
	}

	catalog, err := agents.LoadDefaultCatalog()
	if err != nil {
		return agents.Agent{}, fmt.Errorf("load agent catalog: %w", err)
	}

	agentConfig, ok := catalog.Get(name)
	if !ok {
		return agents.Agent{}, fmt.Errorf("agent %q not found", name)
	}

	return agentConfig, nil
}

func resolveTools(agentConfig agents.Agent) map[string]agent.Tool {
	if agentConfig.AllowedTools == nil {
		return tools.AllTools()
	}
	return tools.GetTools(agentConfig.AllowedTools)
}
