package tools

import (
	"context"
	"fmt"
	"os"
	"path/filepath"
	"regexp"
	"strings"
	"time"

	"github.com/liznear/hh/agent"
)

var nowEditPlan = time.Now

var nonSlugChars = regexp.MustCompile(`[^a-z0-9-]+`)
var repeatedHyphens = regexp.MustCompile(`-+`)
var leadingDatePrefixWithSuffix = regexp.MustCompile(`^\d{4}-\d{2}-\d{2}-(.+)$`)

type EditPlanResult struct {
	Path         string
	OldContent   string
	NewContent   string
	AddedLines   int
	DeletedLines int
}

func (r EditPlanResult) Summary() string {
	if r.AddedLines == 0 && r.DeletedLines == 0 {
		return "no changes"
	}
	return fmt.Sprintf("+%d -%d", r.AddedLines, r.DeletedLines)
}

func NewEditPlanTool() agent.Tool {
	return agent.Tool{
		Name:        "edit_plan",
		Description: "Create or update a plan in ./.hh/plans",
		Schema: map[string]any{
			"type": "object",
			"properties": map[string]any{
				"plan_name": map[string]any{"type": "string"},
				"content":   map[string]any{"type": "string"},
			},
			"required": []string{"plan_name", "content"},
		},
		Handler: agent.FuncToolHandler(handleEditPlan),
	}
}

func handleEditPlan(_ context.Context, params map[string]any) agent.ToolResult {
	planName, err := requiredString(params, "plan_name")
	if err != nil {
		return toolErr("%s", err.Error())
	}
	content, err := optionalString(params, "content")
	if err != nil {
		return toolErr("%s", err.Error())
	}

	slug := slugifyPlanName(planName)
	if slug == "" {
		return toolErr("plan_name must include at least one alphanumeric character")
	}

	today := nowEditPlan().Format("2006-01-02")
	planDir := filepath.Join(".hh", "plans")
	planPath := filepath.Join(planDir, fmt.Sprintf("%s-%s.md", today, slug))

	if err := os.MkdirAll(planDir, 0o755); err != nil {
		return toolErr("failed to create plans directory: %v", err)
	}

	original, readErr := os.ReadFile(planPath)
	if readErr != nil && !os.IsNotExist(readErr) {
		return toolErr("failed to read plan file: %v", readErr)
	}

	mode := os.FileMode(0o644)
	if info, statErr := os.Stat(planPath); statErr == nil {
		mode = info.Mode()
	}

	if err := os.WriteFile(planPath, []byte(content), mode); err != nil {
		return toolErr("failed to write plan file: %v", err)
	}

	oldContent := string(original)
	addedLines, deletedLines := countDiffChanges(oldContent, content)

	return agent.ToolResult{
		Data: "ok",
		Result: EditPlanResult{
			Path:         planPath,
			OldContent:   oldContent,
			NewContent:   content,
			AddedLines:   addedLines,
			DeletedLines: deletedLines,
		},
	}
}

func slugifyPlanName(planName string) string {
	slug := strings.ToLower(strings.TrimSpace(planName))
	slug = strings.ReplaceAll(slug, "_", "-")
	slug = strings.ReplaceAll(slug, " ", "-")
	slug = nonSlugChars.ReplaceAllString(slug, "-")
	slug = repeatedHyphens.ReplaceAllString(slug, "-")
	slug = strings.Trim(slug, "-")
	if matches := leadingDatePrefixWithSuffix.FindStringSubmatch(slug); len(matches) == 2 {
		slug = strings.Trim(matches[1], "-")
	}
	return slug
}
