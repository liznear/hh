package provider

import (
	"context"
	"encoding/json"
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

// Streaming mapping contract (OpenAI chunk -> agent.ProviderResponse):
//
//  1. chunk.choices[0].delta.content
//     -> ProviderResponse.MessageDelta
//
//  2. chunk.choices[0].delta.tool_calls[*]
//     -> ProviderResponse.ToolCallDelta
//        - index -> ToolCallDelta.Index
//        - id -> ToolCallDelta.ID
//        - type -> ToolCallDelta.Type
//        - function.name -> ToolCallDelta.Name
//        - function.arguments -> ToolCallDelta.Arguments
//
//  3. chunk.choices[0].finish_reason
//     -> ProviderResponse.FinishReason
//
//  4. end of stream (accumulated via openai.ChatCompletionAccumulator)
//     -> ProviderResponse.Message (final assistant content)
//     -> ProviderResponse.ToolCalls (final fully assembled tool calls)
//
//  5. stream/transport/decode errors
//     -> ProviderResponse.Error
//
// Notes:
// - We currently map textual delta content to MessageDelta.
// - Tool call deltas can arrive in pieces; final ToolCalls are reconstructed from
//   the accumulator to provide complete arguments.

func NewOpenAICompatibleProvider(baseURL string, apiKey string) agent.Provider {
	return &openAICompatibleProvider{
		client: openai.NewClient(
			option.WithBaseURL(baseURL),
			option.WithAPIKey(apiKey),
		),
	}
}

func (p *openAICompatibleProvider) ChatCompletionStream(ctx context.Context, req agent.ProviderRequest, onEvent func(agent.ProviderStreamEvent) error) (agent.ProviderResponse, error) {
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
	defer resp.Close()

	acc := openai.ChatCompletionAccumulator{}
	var reasoning string

	for resp.Next() {
		chunk := resp.Current()
		if !acc.AddChunk(chunk) {
			return agent.ProviderResponse{}, fmt.Errorf("failed to accumulate streamed chat completion chunk")
		}

		if len(chunk.Choices) == 0 {
			continue
		}

		choice := chunk.Choices[0]

		// Incremental thinking/reasoning text
		if r := extractReasoning(choice); r != "" {
			reasoning += r
			err := onEvent(agent.ProviderStreamEvent{ThinkingDelta: r})
			if err != nil {
				return agent.ProviderResponse{}, err
			}
		}

		// Incremental assistant text.
		if choice.Delta.Content != "" {
			err := onEvent(agent.ProviderStreamEvent{MessageDelta: choice.Delta.Content})
			if err != nil {
				return agent.ProviderResponse{}, err
			}
		}
	}

	if err := resp.Err(); err != nil {
		return agent.ProviderResponse{}, err
	}

	if len(acc.Choices) == 0 {
		return agent.ProviderResponse{}, nil
	}

	choice := acc.Choices[0]

	msg := agent.Message{
		Role:    agent.RoleAssistant,
		Content: choice.Message.Content,
	}

	toolCalls := openAIToAgentToolCall(choice.Message.ToolCalls)

	return agent.ProviderResponse{
		Message:      msg,
		Thinking:     reasoning,
		ToolCalls:    toolCalls,
		FinishReason: agent.FinishReason(choice.FinishReason),
	}, nil
}

func extractReasoning(choice openai.ChatCompletionChunkChoice) string {
	// DeepSeek-style and other common fields
	for _, field := range []string{"reasoning_content", "reasoning"} {
		if reasoningField, ok := choice.Delta.JSON.ExtraFields[field]; ok {
			var reasoning string
			if err := json.Unmarshal([]byte(reasoningField.Raw()), &reasoning); err == nil && reasoning != "" {
				return reasoning
			}
		}
	}
	return ""
}

func toOpenAITool(t *agent.Tool) (openai.ChatCompletionToolUnionParam, bool) {
	if t == nil {
		return openai.ChatCompletionToolUnionParam{}, false
	}

	return openai.ChatCompletionFunctionTool(shared.FunctionDefinitionParam{
		Name:        t.Name,
		Description: openai.Opt(t.Description),
		Parameters:  t.Schema,
	}), true
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

func openAIToAgentToolCall(calls []openai.ChatCompletionMessageToolCallUnion) []agent.ToolCall {
	if len(calls) == 0 {
		return nil
	}
	ret := make([]agent.ToolCall, 0, len(calls))
	for _, call := range calls {
		// openai-go's ChatCompletionAccumulator has a quirk where AsAny() returns an empty struct
		// unless the Union is unmarshaled from JSON. We roundtrip it here to ensure it works.
		b, err := json.Marshal(call)
		if err != nil {
			continue
		}
		var union openai.ChatCompletionMessageToolCallUnion
		if err := json.Unmarshal(b, &union); err != nil {
			continue
		}

		variant := union.AsAny()
		if variant == nil {
			continue
		}

		functionCall, ok := variant.(openai.ChatCompletionMessageFunctionToolCall)
		if !ok {
			continue
		}

		ret = append(ret, agent.ToolCall{
			ID:        functionCall.ID,
			Name:      functionCall.Function.Name,
			Arguments: functionCall.Function.Arguments,
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

var _ agent.Provider = (*openAICompatibleProvider)(nil)
