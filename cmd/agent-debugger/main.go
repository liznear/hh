package main

import (
	"context"
	"encoding/json"
	"flag"
	"fmt"
	"os"

	"github.com/liznear/hh/agent"
	"github.com/liznear/hh/provider"
	"github.com/liznear/hh/tools"
)

func main() {
	providerType := flag.String("provider_type", "openai", "provider type")
	baseURL := flag.String("base_url", os.Getenv("HH_BASE_URL"), "provider base URL")
	apiKey := flag.String("api_key", os.Getenv("HH_API_KEY"), "provider API key")
	model := flag.String("model", "glm-5", "model name")
	prompt := flag.String("prompt", "", "required prompt to run")
	flag.Parse()

	if *prompt == "" {
		fmt.Fprintln(os.Stderr, "missing required flag: -prompt")
		os.Exit(2)
	}

	p, err := buildProvider(*providerType, *baseURL, *apiKey)
	if err != nil {
		fmt.Fprintln(os.Stderr, err)
		os.Exit(2)
	}

	runner := agent.NewAgentRunner(*model, p, agent.WithTools(tools.AllTools()))

	var finalMessages []agent.Message
	runner.Run(context.Background(), agent.Input{Content: *prompt, Type: "text"}, func(e agent.Event) {
		printEvent(e)
		if e.Type != agent.EventTypeAgentEnd {
			return
		}
		if data, ok := e.Data.(agent.EventDataAgentEnd); ok {
			finalMessages = data.Messages
		}
	})

	fmt.Println("FINAL_MESSAGES")
	printJSON(finalMessages)
}

func buildProvider(providerType, baseURL, apiKey string) (agent.Provider, error) {
	switch providerType {
	case "openai":
		return provider.NewOpenAICompatibleProvider(baseURL, apiKey), nil
	default:
		return nil, fmt.Errorf("unsupported provider_type %q", providerType)
	}
}

func printEvent(e agent.Event) {
	printJSON(map[string]any{
		"type": e.Type,
		"data": normalizeEventData(e.Data),
	})
}

func normalizeEventData(data any) any {
	switch v := data.(type) {
	case error:
		return map[string]string{"error": v.Error()}
	case agent.EventDataError:
		if v.Err == nil {
			return map[string]string{"error": ""}
		}
		return map[string]string{"error": v.Err.Error()}
	default:
		return data
	}
}

func printJSON(v any) {
	b, err := json.Marshal(v)
	if err != nil {
		fmt.Fprintf(os.Stderr, "failed to marshal JSON: %v\n", err)
		return
	}
	fmt.Println(string(b))
}
