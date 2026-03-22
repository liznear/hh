package main

import (
	"fmt"
	"os"

	"github.com/liznear/hh/config"
	"github.com/liznear/hh/tui"
)

func main() {
	cfg, err := config.Load()
	if err != nil {
		fmt.Fprintf(os.Stderr, "failed to load hh config: %v\n", err)
		os.Exit(2)
	}

	if err := tui.Run(cfg); err != nil {
		fmt.Fprintf(os.Stderr, "failed to start tui: %v\n", err)
		os.Exit(1)
	}
}
