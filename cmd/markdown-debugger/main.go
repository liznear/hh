package main

import (
	"context"
	"fmt"
	"io"
	"os"
	"strings"

	"github.com/liznear/hh/tui"
	"github.com/urfave/cli/v3"
)

const defaultSampleMarkdown = "# Markdown Debugger\n\n" +
	"This shows **default** vs _thinking_ markdown styles.\n\n" +
	"- List item with `inline code`\n" +
	"- Another item with a [link](https://example.com)\n\n" +
	"> Blockquote with emphasis.\n\n" +
	"```go\n" +
	"func main() {\n" +
	"    fmt.Println(\"hello\")\n" +
	"}\n" +
	"```\n"

func main() {
	cmd := &cli.Command{
		Name:  "markdown-debugger",
		Usage: "render markdown in default and thinking styles",
		Flags: []cli.Flag{
			&cli.IntFlag{
				Name:  "width",
				Value: 80,
				Usage: "render width",
			},
			&cli.StringFlag{
				Name:  "content",
				Usage: "markdown content (if omitted, reads stdin; if stdin empty, uses built-in sample)",
			},
		},
		OnUsageError: func(_ context.Context, _ *cli.Command, err error, _ bool) error {
			return cli.Exit(err.Error(), 2)
		},
		Action: func(_ context.Context, cmd *cli.Command) error {
			width := cmd.Int("width")
			if width <= 0 {
				return cli.Exit("width must be greater than 0", 2)
			}

			content := strings.TrimSpace(cmd.String("content"))
			if content == "" {
				stdin, err := readStdin()
				if err != nil {
					return cli.Exit(fmt.Sprintf("failed to read stdin: %v", err), 1)
				}
				content = strings.TrimSpace(stdin)
			}
			if content == "" {
				content = strings.TrimSpace(defaultSampleMarkdown)
			}

			fmt.Printf("=== default (width=%d) ===\n", width)
			fmt.Println(tui.RenderMarkdownDefault(content, width))
			fmt.Println()
			fmt.Printf("=== thinking (width=%d) ===\n", width)
			fmt.Println(tui.RenderMarkdownThinking(content, width))
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

func readStdin() (string, error) {
	info, err := os.Stdin.Stat()
	if err != nil {
		return "", err
	}
	if info.Mode()&os.ModeCharDevice != 0 {
		return "", nil
	}
	b, err := io.ReadAll(os.Stdin)
	if err != nil {
		return "", err
	}
	return string(b), nil
}
