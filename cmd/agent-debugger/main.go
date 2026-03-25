package main

import (
	"context"
	"encoding/json"
	"fmt"
	"os"
	"strings"

	"github.com/liznear/hh/agent"
	"github.com/liznear/hh/provider"
	"github.com/liznear/hh/skills"
	"github.com/liznear/hh/tools"
	"github.com/urfave/cli/v3"
)

func main() {
	cmd := &cli.Command{
		Name: "agent-debugger",
		Flags: []cli.Flag{
			&cli.StringFlag{
				Name:  "provider_type",
				Value: "openai",
				Usage: "provider type",
			},
			&cli.StringFlag{
				Name:    "base_url",
				Value:   os.Getenv("HH_BASE_URL"),
				Usage:   "provider base URL",
				Sources: cli.EnvVars("HH_BASE_URL"),
			},
			&cli.StringFlag{
				Name:    "api_key",
				Value:   os.Getenv("HH_API_KEY"),
				Usage:   "provider API key",
				Sources: cli.EnvVars("HH_API_KEY"),
			},
			&cli.StringFlag{
				Name:  "model",
				Value: "glm-5",
				Usage: "model name",
			},
			&cli.StringFlag{
				Name:     "prompt",
				Usage:    "required prompt to run",
				Required: true,
			},
		},
		OnUsageError: func(_ context.Context, _ *cli.Command, err error, _ bool) error {
			return cli.Exit(err.Error(), 2)
		},
		Action: func(ctx context.Context, cmd *cli.Command) error {
			p, err := buildProvider(cmd.String("provider_type"), cmd.String("base_url"), cmd.String("api_key"))
			if err != nil {
				return cli.Exit(err.Error(), 2)
			}

			skillCatalog, err := skills.LoadDefaultCatalog()
			if err != nil {
				return cli.Exit(fmt.Sprintf("failed to load skills: %v", err), 1)
			}
			tools.SetSkillCatalog(skillCatalog)
			systemPrompt := strings.TrimSpace(skillCatalog.PromptFrontmatterBlock())

			runner := agent.NewAgentRunner(cmd.String("model"), p, agent.WithTools(tools.AllTools()), agent.WithSystemPrompt(systemPrompt))

			var finalMessages []agent.Message
			if err := runner.Run(ctx, agent.Input{Content: cmd.String("prompt"), Type: "text"}, func(e agent.Event) {
				printEvent(e)
				if e.Type != agent.EventTypeAgentEnd {
					return
				}
				if data, ok := e.Data.(agent.EventDataAgentEnd); ok {
					finalMessages = data.Messages
				}
			}); err != nil {
				return cli.Exit(fmt.Sprintf("failed to run agent: %v", err), 1)
			}

			fmt.Println("FINAL_MESSAGES")
			printJSON(finalMessages)
			return nil
		},
	}

	if err := cmd.Run(context.Background(), os.Args); err != nil {
		if exitErr, ok := err.(interface{ ExitCode() int }); ok {
			if msg := strings.TrimSpace(err.Error()); msg != "" {
				fmt.Fprintln(os.Stderr, msg)
			}
			os.Exit(exitErr.ExitCode())
		}
		fmt.Fprintln(os.Stderr, err)
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
