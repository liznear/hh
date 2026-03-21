package main

import (
	"flag"
	"fmt"
	"os"

	"github.com/liznear/hh/agent"
	"github.com/liznear/hh/provider"
	"github.com/liznear/hh/tools"
	"github.com/liznear/hh/tui"
)

func main() {
	providerType := flag.String("provider_type", "openai", "provider type")
	baseURL := flag.String("base_url", os.Getenv("HH_BASE_URL"), "provider base URL")
	apiKey := flag.String("api_key", os.Getenv("HH_API_KEY"), "provider API key")
	model := flag.String("model", "glm-5", "model name")
	flag.Parse()

	p, err := buildProvider(*providerType, *baseURL, *apiKey)
	if err != nil {
		fmt.Fprintln(os.Stderr, err)
		os.Exit(2)
	}

	runner := agent.NewAgentRunner(*model, p, agent.WithTools(tools.AllTools()))
	if err := tui.Run(runner, *model); err != nil {
		fmt.Fprintf(os.Stderr, "failed to start tui: %v\n", err)
		os.Exit(1)
	}
}

func buildProvider(providerType, baseURL, apiKey string) (agent.Provider, error) {
	switch providerType {
	case "openai":
		return provider.NewOpenAICompatibleProvider(baseURL, apiKey), nil
	default:
		return nil, fmt.Errorf("unsupported provider_type %q", providerType)
	}
}
