package agent

import (
	"context"
)

type ProviderRequest struct {
	Model    string
	Messages []Message
	Tools    []Tool
}

type ProviderResponse struct {
	Message   Message
	ToolCalls []ToolCall
}

type Provider interface {
	ChatCompletionStream(ctx context.Context, req ProviderRequest) (chan ProviderResponse, error)
}
