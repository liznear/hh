package agent

import (
	"context"
)

type ProviderRequest struct {
	Model    string
	Messages []Message
	Tools    []Tool
}

type ProviderStreamEvent struct {
	ThinkingDelta string
	MessageDelta  string
	ToolCallDelta *ToolCallDelta
}

type ProviderResponse struct {
	Message      Message
	Thinking     string
	ToolCalls    []ToolCall
	FinishReason FinishReason
}

type ToolCallDelta struct {
	Index     int
	ID        string
	Type      ToolCallType
	Name      string
	Arguments string
}

type FinishReason string

const (
	FinishReasonUnknown       FinishReason = ""
	FinishReasonStop          FinishReason = "stop"
	FinishReasonLength        FinishReason = "length"
	FinishReasonToolCalls     FinishReason = "tool_calls"
	FinishReasonContentFilter FinishReason = "content_filter"
	FinishReasonFunctionCall  FinishReason = "function_call"
)

type Provider interface {
	ChatCompletionStream(ctx context.Context, req ProviderRequest, onEvent func(ProviderStreamEvent) error) (ProviderResponse, error)
}
