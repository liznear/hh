package tools

import (
	"context"
	"fmt"
	"strings"
	"sync"

	"github.com/liznear/hh/agent"
	"github.com/liznear/hh/skills"
)

type SkillResult struct {
	Name     string
	Location string
}

func (r SkillResult) Summary() string {
	if r.Name == "" {
		return "skill loaded"
	}
	return fmt.Sprintf("skill %q loaded", r.Name)
}

func NewSkillTool() agent.Tool {
	return agent.Tool{
		Name:        "skill",
		Description: "Load a skill by name",
		Schema: map[string]any{
			"type": "object",
			"properties": map[string]any{
				"name": map[string]any{"type": "string"},
			},
			"required": []string{"name"},
		},
		Handler: agent.FuncToolHandler(handleSkill),
	}
}

func handleSkill(_ context.Context, params map[string]any) agent.ToolResult {
	name, err := requiredString(params, "name")
	if err != nil {
		return toolErr("%s", err.Error())
	}

	catalog, err := loadSkillCatalog()
	if err != nil {
		return toolErr("failed to load skills catalog: %v", err)
	}

	if catalog.IsEmpty() {
		return toolErr("no skills available")
	}

	entry, ok := catalog.SkillByName(name)
	if !ok {
		available := make([]string, 0, len(catalog.Entries()))
		for _, skill := range catalog.Entries() {
			available = append(available, skill.Name)
		}
		return toolErr("skill %q not found; available skills: %s", name, strings.Join(available, ", "))
	}

	return agent.ToolResult{
		Data: fmt.Sprintf("<skill_content name=\"%s\">\n%s\n</skill_content>", entry.Name, entry.Content),
		Result: SkillResult{
			Name:     entry.Name,
			Location: entry.Location,
		},
	}
}

func SetSkillCatalog(catalog skills.Catalog) {
	skillCatalogMu.Lock()
	defer skillCatalogMu.Unlock()
	skillCatalog = &catalog
	skillCatalogErr = nil
}

func loadSkillCatalog() (skills.Catalog, error) {
	skillCatalogMu.RLock()
	if skillCatalog != nil {
		ret := *skillCatalog
		err := skillCatalogErr
		skillCatalogMu.RUnlock()
		return ret, err
	}
	skillCatalogMu.RUnlock()

	catalog, err := skills.LoadDefaultCatalog()
	skillCatalogMu.Lock()
	skillCatalog = &catalog
	skillCatalogErr = err
	skillCatalogMu.Unlock()
	return catalog, err
}

var (
	skillCatalogMu  sync.RWMutex
	skillCatalog    *skills.Catalog
	skillCatalogErr error
)
