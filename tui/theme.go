package tui

import "github.com/charmbracelet/lipgloss"

type Base16Palette struct {
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

func TerminalBase16Palette() Base16Palette {
	return Base16Palette{
		Base00: lipgloss.Color("0"),
		Base01: lipgloss.Color("8"),
		Base02: lipgloss.Color("0"),
		Base03: lipgloss.Color("8"),
		Base04: lipgloss.Color("7"),
		Base05: lipgloss.Color("7"),
		Base06: lipgloss.Color("15"),
		Base07: lipgloss.Color("15"),
		Base08: lipgloss.Color("1"),
		Base09: lipgloss.Color("3"),
		Base0A: lipgloss.Color("3"),
		Base0B: lipgloss.Color("2"),
		Base0C: lipgloss.Color("6"),
		Base0D: lipgloss.Color("4"),
		Base0E: lipgloss.Color("5"),
		Base0F: lipgloss.Color("1"),
	}
}

type Theme struct {
	palette Base16Palette
}

func NewTheme(palette Base16Palette) Theme {
	return Theme{palette: palette}
}

func DefaultTheme() Theme {
	return NewTheme(TerminalBase16Palette())
}

func (t Theme) Background() lipgloss.Color {
	return t.palette.Base00
}

func (t Theme) Surface() lipgloss.Color {
	return t.palette.Base01
}

func (t Theme) Foreground() lipgloss.Color {
	return t.palette.Base05
}

func (t Theme) Emphasis() lipgloss.Color {
	return t.palette.Base06
}

func (t Theme) Muted() lipgloss.Color {
	return t.palette.Base03
}

func (t Theme) Error() lipgloss.Color {
	return t.palette.Base08
}

func (t Theme) Warning() lipgloss.Color {
	return t.palette.Base09
}

func (t Theme) Success() lipgloss.Color {
	return t.palette.Base0B
}

func (t Theme) Info() lipgloss.Color {
	return t.palette.Base0D
}
