package main

import (
	"context"
	"errors"
	"fmt"
	"io"
	"os"
	"strings"

	"github.com/liznear/hh/config"
	"github.com/liznear/hh/tui"
	"github.com/urfave/cli/v3"
)

var version = "dev"

func main() {
	os.Exit(run(os.Args[1:], os.Stdout, os.Stderr))
}

func run(args []string, stdout, stderr io.Writer) int {
	cmd := &cli.Command{
		Name:            "hh",
		HideHelpCommand: true,
		Flags: []cli.Flag{
			&cli.BoolFlag{
				Name:    "version",
				Aliases: []string{"v"},
				Usage:   "show version and exit",
			},
		},
		OnUsageError: func(_ context.Context, _ *cli.Command, err error, _ bool) error {
			return cli.Exit(err.Error(), 2)
		},
		Action: func(_ context.Context, cmd *cli.Command) error {
			if cmd.Bool("version") {
				fmt.Fprintf(stdout, "hh %s\n", strings.TrimSpace(version))
				return nil
			}

			if cmd.NArg() > 0 {
				return cli.Exit(fmt.Sprintf("unexpected arguments: %s", strings.Join(cmd.Args().Slice(), " ")), 2)
			}

			cfg, err := config.Load()
			if err != nil {
				return cli.Exit(fmt.Sprintf("failed to load hh config: %v", err), 2)
			}

			if err := tui.Run(cfg); err != nil {
				return cli.Exit(fmt.Sprintf("failed to start tui: %v", err), 1)
			}

			return nil
		},
	}
	cmd.Writer = stdout
	cmd.ErrWriter = stderr

	err := cmd.Run(context.Background(), append([]string{"hh"}, args...))
	if err == nil {
		return 0
	}

	var exitCoder interface{ ExitCode() int }
	if errors.As(err, &exitCoder) {
		if msg := strings.TrimSpace(err.Error()); msg != "" {
			fmt.Fprintln(stderr, msg)
		}
		return exitCoder.ExitCode()
	}

	fmt.Fprintln(stderr, err)
	return 1
}
