package provider

import (
	"context"
	"fmt"

	"github.com/liznear/hh/agent"
	"github.com/openai/openai-go/v3"
	"github.com/openai/openai-go/v3/option"
	"github.com/openai/openai-go/v3/shared"
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
		Tools: lo.FilterMap(req.Tools, func(t agent.Tool, _ int) (openai.ChatCompletionToolUnionParam, bool) {
			tool, ok := toOpenAITool(&t)
			return tool, ok
		}),
	})
	ch := make(chan agent.ProviderResponse)
	go func() {
		defer close(ch)
		defer resp.Close()

		acc := openai.ChatCompletionAccumulator{}
		for resp.Next() {
			chunk := resp.Current()
			if !acc.AddChunk(chunk) {
				ch <- agent.ProviderResponse{Error: fmt.Errorf("failed to accumulate streamed chat completion chunk")}
				return
			}

			if len(chunk.Choices) == 0 {
				continue
			}

			choice := chunk.Choices[0]
			if choice.Delta.Content != "" {
				ch <- agent.ProviderResponse{MessageDelta: choice.Delta.Content}
			}

			for _, tc := range choice.Delta.ToolCalls {
				ch <- agent.ProviderResponse{
					ToolCallDelta: &agent.ToolCallDelta{
						Index:     clampToZero(tc.Index),
						ID:        tc.ID,
						Type:      agent.ToolCallType(tc.Type),
						Name:      tc.Function.Name,
						Arguments: tc.Function.Arguments,
					},
				}
			}

			if choice.FinishReason != "" {
				ch <- agent.ProviderResponse{FinishReason: agent.FinishReason(choice.FinishReason)}
			}
		}

		if err := resp.Err(); err != nil {
			ch <- agent.ProviderResponse{Error: err}
			return
		}

		if len(acc.Choices) == 0 {
			return
		}

		msg := agent.Message{
			Role:    agent.RoleAssistant,
			Content: acc.Choices[0].Message.Content,
		}
		ch <- agent.ProviderResponse{Message: &msg}

		toolCalls := toAgentToolCalls(acc.Choices[0].Message.ToolCalls)
		if len(toolCalls) > 0 {
			ch <- agent.ProviderResponse{ToolCalls: toolCalls}
		}
	}()
	return ch, nil
}

func toOpenAITool(t *agent.Tool) (openai.ChatCompletionToolUnionParam, bool) {
	if t == nil {
		return openai.ChatCompletionToolUnionParam{}, false
	}

	if t.Type == agent.ToolTypeFunction && t.Function.Name != "" {
		return openai.ChatCompletionFunctionTool(shared.FunctionDefinitionParam{
			Name:        t.Function.Name,
			Description: openai.Opt(t.Function.Description),
			Parameters:  t.Function.Parameters,
		}), true
	}

	return openai.ChatCompletionToolUnionParam{}, false
}

func toAgentToolCalls(calls []openai.ChatCompletionMessageToolCallUnion) []agent.ToolCall {
	ret := make([]agent.ToolCall, 0, len(calls))
	for _, call := range calls {
		variant := call.AsAny()
		if variant == nil {
			continue
		}

		functionCall, ok := variant.(openai.ChatCompletionMessageFunctionToolCall)
		if !ok {
			continue
		}

		ret = append(ret, agent.ToolCall{
			ID:   functionCall.ID,
			Type: agent.ToolCallType(functionCall.Type),
			Function: agent.ToolCallFunction{
				Name:      functionCall.Function.Name,
				Arguments: functionCall.Function.Arguments,
			},
		})
	}
	return ret
}

func clampToZero(index int64) int {
	if index < 0 {
		return 0
	}
	return int(index)
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
