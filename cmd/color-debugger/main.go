package main

import (
	"charm.land/lipgloss/v2"
	"fmt"
	"image/color"

	"github.com/liznear/hh/tui"
)

const blockWidth = 3

func main() {
	palette := tui.TerminalBase16Palette()
	entries := []struct {
		name  string
		color color.Color
	}{
		{name: "Base00", color: palette.Base00},
		{name: "Base01", color: palette.Base01},
		{name: "Base02", color: palette.Base02},
		{name: "Base03", color: palette.Base03},
		{name: "Base04", color: palette.Base04},
		{name: "Base05", color: palette.Base05},
		{name: "Base06", color: palette.Base06},
		{name: "Base07", color: palette.Base07},
		{name: "Base08", color: palette.Base08},
		{name: "Base09", color: palette.Base09},
		{name: "Base0A", color: palette.Base0A},
		{name: "Base0B", color: palette.Base0B},
		{name: "Base0C", color: palette.Base0C},
		{name: "Base0D", color: palette.Base0D},
		{name: "Base0E", color: palette.Base0E},
		{name: "Base0F", color: palette.Base0F},
	}

	for _, entry := range entries {
		block := lipgloss.NewStyle().
			Background(entry.color).
			Width(blockWidth).
			Render("")
		fmt.Printf("%s %s\n", entry.name, block)
	}
}
