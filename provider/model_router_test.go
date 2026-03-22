package provider

import (
	"context"
	"testing"

	"github.com/liznear/hh/agent"
)

type recordingProvider struct {
	lastModel string
}

func (p *recordingProvider) ChatCompletionStream(_ context.Context, req agent.ProviderRequest, _ func(agent.ProviderStreamEvent) error) (agent.ProviderResponse, error) {
	p.lastModel = req.Model
	return agent.ProviderResponse{}, nil
}

func TestModelRouterProvider_RoutesConfiguredModel(t *testing.T) {
	target := &recordingProvider{}
	p := NewModelRouterProvider(map[string]ModelRoute{
		"proxy/gpt-5.3-codex": {
			Provider: target,
			Model:    "gpt-5.3-codex",
		},
	})

	_, err := p.ChatCompletionStream(context.Background(), agent.ProviderRequest{Model: "proxy/gpt-5.3-codex"}, func(agent.ProviderStreamEvent) error {
		return nil
	})
	if err != nil {
		t.Fatalf("ChatCompletionStream() error = %v", err)
	}
	if target.lastModel != "gpt-5.3-codex" {
		t.Fatalf("routed model = %q, want %q", target.lastModel, "gpt-5.3-codex")
	}
}

func TestModelRouterProvider_ReturnsErrorWhenRouteMissing(t *testing.T) {
	p := NewModelRouterProvider(map[string]ModelRoute{})

	_, err := p.ChatCompletionStream(context.Background(), agent.ProviderRequest{Model: "glm-5"}, func(agent.ProviderStreamEvent) error {
		return nil
	})
	if err == nil {
		t.Fatal("ChatCompletionStream() error = nil, want non-nil")
	}
}
