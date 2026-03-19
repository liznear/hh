package provider

import (
	"context"
	"fmt"

	"github.com/liznear/hh/agent"
	"github.com/openai/openai-go/v3"
	"github.com/openai/openai-go/v3/option"
	"github.com/samber/lo"
)

type openAICompatibleProvider struct {
	client openai.Client
}

func NewOpenAICompatibleProvider(baseURL string, apiKey string) agent.Provider {
	return &openAICompatibleProvider{
		client: openai.NewClient(
			option.WithBaseURL(baseURL),
			option.WithAPIKey(apiKey),
		),
	}
}

func (p *openAICompatibleProvider) ChatCompletionStream(ctx context.Context, req agent.ProviderRequest) (chan agent.ProviderResponse, error) {
	resp := p.client.Chat.Completions.NewStreaming(ctx, openai.ChatCompletionNewParams{
		Model: openai.ChatModel(req.Model),
		Messages: lo.Map(req.Messages, func(m agent.Message, idx int) openai.ChatCompletionMessageParamUnion {
			return toOpenAIMessage(&m)
		}),
	})
	ch := make(chan agent.ProviderResponse)
	go func() {
		defer close(ch)
		for resp.Next() {
			event := resp.Current()
			fmt.Printf("%v\n", event)
		}
	}()
	return ch, nil
}

func toOpenAIMessage(m *agent.Message) openai.ChatCompletionMessageParamUnion {
	switch m.Role {
	case agent.RoleSystem:
		return openai.SystemMessage(m.Content)
	case agent.RoleUser:
		return openai.UserMessage(m.Content)
	case agent.RoleAssistant:
		return openai.AssistantMessage(m.Content)
	case agent.RoleTool:
		return openai.ToolMessage(m.Content, m.CallID)
	default:
		return openai.UserMessage(m.Content)
	}
}

var _ agent.Provider = (*openAICompatibleProvider)(nil)
