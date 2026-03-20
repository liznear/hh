package provider

import (
	"context"
	"fmt"
	"os"
	"path/filepath"
	"reflect"
	"testing"

	"github.com/liznear/hh/agent"
)

func Test_OpenAIChatCompletionStream(t *testing.T) {
	sessionName := "weather"
	providerName := "openai"

	server := startMockSessionServer(t, sessionName, providerName)
	defer server.Close()

	// OpenAI client will append /chat/completions to the base URL
	p := NewOpenAICompatibleProvider(server.URL+"/v1", "test-api-key")

	ctx := context.Background()

	for step := 1; ; step++ {
		reqFile := filepath.Join("testdata", "sessions", sessionName, fmt.Sprintf("req-%d.json", step))
		if _, err := os.Stat(reqFile); os.IsNotExist(err) {
			break
		}

		req := loadProviderRequest(t, sessionName, step)
		want := loadExpectedEvents(t, sessionName, step)

		ch, err := p.ChatCompletionStream(ctx, req)
		if err != nil {
			t.Fatalf("fail to call ChatCompletionStream (step %d): %v", step, err)
		}

		var got []agent.ProviderResponse
		for resp := range ch {
			if resp.Error != nil {
				t.Fatalf("stream error step %d: %v", step, resp.Error)
			}
			got = append(got, resp)
		}

		if !reflect.DeepEqual(got, want) {
			t.Fatalf("step %d responses mismatch\nGot:  %#v\nWant: %#v", step, got, want)
		}
	}
}
