package provider

import (
	"context"
	"fmt"

	"github.com/liznear/hh/agent"
)

type ModelRoute struct {
	Provider agent.Provider
	Model    string
}

type modelRouterProvider struct {
	routes map[string]ModelRoute
}

func NewModelRouterProvider(routes map[string]ModelRoute) agent.Provider {
	cloned := make(map[string]ModelRoute, len(routes))
	for modelName, route := range routes {
		cloned[modelName] = route
	}
	return &modelRouterProvider{routes: cloned}
}

func (p *modelRouterProvider) ChatCompletionStream(ctx context.Context, req agent.ProviderRequest, onEvent func(agent.ProviderStreamEvent) error) (agent.ProviderResponse, error) {
	route, ok := p.routes[req.Model]
	if ok && route.Provider != nil {
		routedReq := req
		if route.Model != "" {
			routedReq.Model = route.Model
		}
		return route.Provider.ChatCompletionStream(ctx, routedReq, onEvent)
	}
	return agent.ProviderResponse{}, fmt.Errorf("no provider route for model %q", req.Model)
}

var _ agent.Provider = (*modelRouterProvider)(nil)
