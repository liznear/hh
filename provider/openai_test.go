package provider

import (
	"context"
	"os"
	"testing"

	"github.com/liznear/hh/agent"
)

func Test_OpenAIChatCompletionStream(t *testing.T) {
	p := NewOpenAICompatibleProvider("http://localhost:8317/v1", os.Getenv("LOCAL_PROXY_API_KEY"))
	ch, err := p.ChatCompletionStream(context.Background(), agent.ProviderRequest{
		Model: "glm-5",
		Messages: []agent.Message{
			{
				Role:    agent.RoleSystem,
				Content: "Hi",
			},
			{
				Role:    agent.RoleUser,
				Content: "Hi",
			},
		},
	})
	if err != nil {
		t.Fatalf("fail to call ChatCompletionStream: %v", err)
	}
	for range ch {
		return
	}
	t.Fail()
}
