package tui

import "github.com/charmbracelet/lipgloss"

type Base16Theme struct {
	Base00 lipgloss.Color
	Base01 lipgloss.Color
	Base02 lipgloss.Color
	Base03 lipgloss.Color
	Base04 lipgloss.Color
	Base05 lipgloss.Color
	Base06 lipgloss.Color
	Base07 lipgloss.Color
	Base08 lipgloss.Color
	Base09 lipgloss.Color
	Base0A lipgloss.Color
	Base0B lipgloss.Color
	Base0C lipgloss.Color
	Base0D lipgloss.Color
	Base0E lipgloss.Color
	Base0F lipgloss.Color
}

func DefaultBase16Theme() Base16Theme {
	return Base16Theme{
		Base00: lipgloss.Color("#181818"),
		Base01: lipgloss.Color("#282828"),
		Base02: lipgloss.Color("#383838"),
		Base03: lipgloss.Color("#585858"),
		Base04: lipgloss.Color("#b8b8b8"),
		Base05: lipgloss.Color("#d8d8d8"),
		Base06: lipgloss.Color("#e8e8e8"),
		Base07: lipgloss.Color("#f8f8f8"),
		Base08: lipgloss.Color("#ab4642"),
		Base09: lipgloss.Color("#dc9656"),
		Base0A: lipgloss.Color("#f7ca88"),
		Base0B: lipgloss.Color("#a1b56c"),
		Base0C: lipgloss.Color("#86c1b9"),
		Base0D: lipgloss.Color("#7cafc2"),
		Base0E: lipgloss.Color("#ba8baf"),
		Base0F: lipgloss.Color("#a16946"),
	}
}
