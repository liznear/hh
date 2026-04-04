package tools

import (
	"context"
	"os"
	"path/filepath"
	"testing"
	"time"
)

func TestEditPlanTool_CreatesPlanFile(t *testing.T) {
	tmpDir := t.TempDir()
	oldWd, err := os.Getwd()
	if err != nil {
		t.Fatalf("getwd: %v", err)
	}
	defer func() {
		_ = os.Chdir(oldWd)
	}()
	if err := os.Chdir(tmpDir); err != nil {
		t.Fatalf("chdir temp dir: %v", err)
	}

	oldNow := nowEditPlan
	nowEditPlan = func() time.Time { return time.Date(2026, 3, 28, 12, 0, 0, 0, time.UTC) }
	defer func() {
		nowEditPlan = oldNow
	}()

	args := map[string]any{
		"plan_name": "New Feature Plan",
		"content":   "# Plan\n\n- Step 1\n",
	}

	res := NewEditPlanTool().Handler.Handle(context.Background(), args)
	if res.IsErr {
		t.Fatalf("expected success, got error: %s", res.Data)
	}

	expectedPath := filepath.Join(".hh", "plans", "2026-03-28-new-feature-plan.md")
	content, err := os.ReadFile(expectedPath)
	if err != nil {
		t.Fatalf("read created plan: %v", err)
	}
	if string(content) != "# Plan\n\n- Step 1\n" {
		t.Fatalf("unexpected plan content: %q", string(content))
	}

	structured, ok := res.Result.(EditPlanResult)
	if !ok {
		t.Fatalf("unexpected result type: %T", res.Result)
	}
	if structured.Path != expectedPath {
		t.Fatalf("unexpected path: got %q want %q", structured.Path, expectedPath)
	}
	if structured.AddedLines != 3 || structured.DeletedLines != 0 {
		t.Fatalf("unexpected diff counts: %+v", structured)
	}
}

func TestEditPlanTool_UpdatesExistingPlanFile(t *testing.T) {
	tmpDir := t.TempDir()
	oldWd, err := os.Getwd()
	if err != nil {
		t.Fatalf("getwd: %v", err)
	}
	defer func() {
		_ = os.Chdir(oldWd)
	}()
	if err := os.Chdir(tmpDir); err != nil {
		t.Fatalf("chdir temp dir: %v", err)
	}

	oldNow := nowEditPlan
	nowEditPlan = func() time.Time { return time.Date(2026, 3, 28, 12, 0, 0, 0, time.UTC) }
	defer func() {
		nowEditPlan = oldNow
	}()

	planPath := filepath.Join(".hh", "plans", "2026-03-28-upgrade-plan.md")
	if err := os.MkdirAll(filepath.Dir(planPath), 0o755); err != nil {
		t.Fatalf("mkdir plan dir: %v", err)
	}
	if err := os.WriteFile(planPath, []byte("# Old\n- a\n"), 0o644); err != nil {
		t.Fatalf("seed plan: %v", err)
	}

	args := map[string]any{
		"plan_name": "Upgrade Plan",
		"content":   "# New\n- b\n",
	}

	res := NewEditPlanTool().Handler.Handle(context.Background(), args)
	if res.IsErr {
		t.Fatalf("expected success, got error: %s", res.Data)
	}

	content, err := os.ReadFile(planPath)
	if err != nil {
		t.Fatalf("read updated plan: %v", err)
	}
	if string(content) != "# New\n- b\n" {
		t.Fatalf("unexpected updated content: %q", string(content))
	}

	structured, ok := res.Result.(EditPlanResult)
	if !ok {
		t.Fatalf("unexpected result type: %T", res.Result)
	}
	if structured.AddedLines == 0 || structured.DeletedLines == 0 {
		t.Fatalf("expected non-zero edit counts: %+v", structured)
	}
}

func TestEditPlanTool_StripsDatePrefixFromPlanName(t *testing.T) {
	tmpDir := t.TempDir()
	oldWd, err := os.Getwd()
	if err != nil {
		t.Fatalf("getwd: %v", err)
	}
	defer func() {
		_ = os.Chdir(oldWd)
	}()
	if err := os.Chdir(tmpDir); err != nil {
		t.Fatalf("chdir temp dir: %v", err)
	}

	oldNow := nowEditPlan
	nowEditPlan = func() time.Time { return time.Date(2026, 3, 28, 12, 0, 0, 0, time.UTC) }
	defer func() {
		nowEditPlan = oldNow
	}()

	args := map[string]any{
		"plan_name": "2026-03-28 New Feature Plan",
		"content":   "# Plan\n",
	}

	res := NewEditPlanTool().Handler.Handle(context.Background(), args)
	if res.IsErr {
		t.Fatalf("expected success, got error: %s", res.Data)
	}

	expectedPath := filepath.Join(".hh", "plans", "2026-03-28-new-feature-plan.md")
	if _, err := os.Stat(expectedPath); err != nil {
		t.Fatalf("expected plan at %q: %v", expectedPath, err)
	}

	unexpectedPath := filepath.Join(".hh", "plans", "2026-03-28-2026-03-28-new-feature-plan.md")
	if _, err := os.Stat(unexpectedPath); !os.IsNotExist(err) {
		t.Fatalf("unexpected duplicated-date plan path exists: %q", unexpectedPath)
	}

	structured, ok := res.Result.(EditPlanResult)
	if !ok {
		t.Fatalf("unexpected result type: %T", res.Result)
	}
	if structured.Path != expectedPath {
		t.Fatalf("unexpected path: got %q want %q", structured.Path, expectedPath)
	}
}

func TestEditPlanTool_InvalidPlanName(t *testing.T) {
	args := map[string]any{
		"plan_name": "!!!",
		"content":   "content",
	}

	res := NewEditPlanTool().Handler.Handle(context.Background(), args)
	if !res.IsErr {
		t.Fatal("expected error for invalid plan name")
	}
}
