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
		wantEvents := loadExpectedEvents(t, sessionName, step)
		wantResponse := loadExpectedResponse(t, sessionName, step)

		var gotEvents []agent.ProviderStreamEvent
		gotResponse, err := p.ChatCompletionStream(ctx, req, func(e agent.ProviderStreamEvent) error {
			gotEvents = append(gotEvents, e)
			return nil
		})

		if err != nil {
			t.Fatalf("fail to call ChatCompletionStream (step %d): %v", step, err)
		}

		if !reflect.DeepEqual(gotEvents, wantEvents) {
			t.Fatalf("step %d events mismatch\nGot:  %#v\nWant: %#v", step, gotEvents, wantEvents)
		}

		if !reflect.DeepEqual(gotResponse, wantResponse) {
			t.Fatalf("step %d response mismatch\nGot:  %#v\nWant: %#v", step, gotResponse, wantResponse)
		}
	}
}
