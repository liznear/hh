package tui

import "testing"

func TestGetAgent_DefaultBuild(t *testing.T) {
	agentConfig, err := getAgent("")
	if err != nil {
		t.Fatalf("expected default agent, got error: %v", err)
	}
	if agentConfig.Name != "Build" {
		t.Fatalf("expected default agent Build, got %q", agentConfig.Name)
	}
}

func TestGetAgent_NotFound(t *testing.T) {
	_, err := getAgent("missing")
	if err == nil {
		t.Fatalf("expected error for unknown agent")
	}
}
