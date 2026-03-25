package agents

import (
	"embed"
	"fmt"
	"sort"
	"strings"
	"sync"
)

//go:embed *.md
var agentFiles embed.FS

type Agent struct {
	Name         string
	SystemPrompt string
	AllowedTools []string
}

type Catalog struct {
	agents map[string]Agent
}

func (c Catalog) Get(name string) (Agent, bool) {
	agent, ok := c.agents[name]
	return agent, ok
}

func (c Catalog) All() []Agent {
	result := make([]Agent, 0, len(c.agents))
	for _, agent := range c.agents {
		result = append(result, agent)
	}
	sort.Slice(result, func(i, j int) bool {
		return result[i].Name < result[j].Name
	})
	return result
}

var (
	defaultCatalogOnce sync.Once
	defaultCatalog     Catalog
	defaultCatalogErr  error
)

func LoadDefaultCatalog() (Catalog, error) {
	defaultCatalogOnce.Do(func() {
		defaultCatalog, defaultCatalogErr = loadFromEmbed()
	})
	return defaultCatalog, defaultCatalogErr
}

func loadFromEmbed() (Catalog, error) {
	entries, err := agentFiles.ReadDir(".")
	if err != nil {
		return Catalog{}, fmt.Errorf("read embedded agents dir: %w", err)
	}

	agentsMap := make(map[string]Agent)
	for _, entry := range entries {
		if entry.IsDir() || !strings.HasSuffix(entry.Name(), ".md") {
			continue
		}

		content, err := agentFiles.ReadFile(entry.Name())
		if err != nil {
			return Catalog{}, fmt.Errorf("read agent file %s: %w", entry.Name(), err)
		}

		agent, err := parseAgentMarkdown(string(content))
		if err != nil {
			return Catalog{}, fmt.Errorf("parse agent file %s: %w", entry.Name(), err)
		}

		if agent.Name == "" {
			return Catalog{}, fmt.Errorf("agent file %s missing name field", entry.Name())
		}

		agentsMap[agent.Name] = agent
	}

	return Catalog{agents: agentsMap}, nil
}

func parseAgentMarkdown(content string) (Agent, error) {
	frontmatter, body := extractFrontmatter(content)
	fields := parseFrontmatterFields(frontmatter)

	name := strings.TrimSpace(fields["name"])
	if name == "" {
		return Agent{}, fmt.Errorf("missing required field: name")
	}

	var allowedTools []string
	if toolsStr := strings.TrimSpace(fields["allowed_tools"]); toolsStr != "" {
		// Parse as comma-separated list
		for _, tool := range strings.Split(toolsStr, ",") {
			tool = strings.TrimSpace(tool)
			if tool != "" {
				allowedTools = append(allowedTools, tool)
			}
		}
	}

	return Agent{
		Name:         name,
		SystemPrompt: strings.TrimSpace(body),
		AllowedTools: allowedTools,
	}, nil
}

func extractFrontmatter(content string) (string, string) {
	content = strings.TrimSpace(content)
	if !strings.HasPrefix(content, "---\n") && !strings.HasPrefix(content, "---\r\n") {
		return "", content
	}

	startDelimiterLen := len("---\n")
	if strings.HasPrefix(content, "---\r\n") {
		startDelimiterLen = len("---\r\n")
	}

	rest := content[startDelimiterLen:]
	end := strings.Index(rest, "\n---")
	if end < 0 {
		return "", content
	}

	frontmatter := strings.TrimSpace(rest[:end])
	body := strings.TrimSpace(rest[end+4:])
	return frontmatter, body
}

func parseFrontmatterFields(raw string) map[string]string {
	out := map[string]string{}
	if strings.TrimSpace(raw) == "" {
		return out
	}

	for _, line := range strings.Split(raw, "\n") {
		line = strings.TrimSpace(line)
		if line == "" || strings.HasPrefix(line, "#") {
			continue
		}
		key, value, ok := strings.Cut(line, ":")
		if !ok {
			continue
		}
		key = strings.ToLower(strings.TrimSpace(key))
		if key == "" {
			continue
		}
		value = strings.TrimSpace(value)
		value = strings.Trim(value, "\"'")
		out[key] = value
	}

	return out
}
