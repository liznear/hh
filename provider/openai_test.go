package provider

import (
	"context"
	"fmt"
	"os"
	"testing"
	"time"

	"github.com/liznear/hh/agent"
)

func Test_OpenAIChatCompletionStream(t *testing.T) {
	p := NewOpenAICompatibleProvider("http://localhost:8317/v1", os.Getenv("LOCAL_PROXY_API_KEY"))
	ch, err := p.ChatCompletionStream(context.Background(), agent.ProviderRequest{
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
	time.Sleep(10 * time.Second)
	for resp := range ch {
		fmt.Printf("%v", resp)
	}
	t.Fail()
}
