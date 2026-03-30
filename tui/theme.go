package tui

import (
	"charm.land/lipgloss/v2"
	"image/color"
)

const (
	ThemeColorShellMessageBackground        = "shell_message_background"
	ThemeColorStatusSpinnerForeground       = "status_spinner_foreground"
	ThemeColorStatusDurationForeground      = "status_duration_foreground"
	ThemeColorStatusInterruptHintForeground = "status_interrupt_hint_foreground"
	ThemeColorUserMessageBorderForeground   = "user_message_border_foreground"
	ThemeColorThinkingForeground            = "thinking_foreground"
	ThemeColorTurnFooterForeground          = "turn_footer_foreground"
	ThemeColorToolCallIconSuccessForeground = "tool_call_icon_success_foreground"
	ThemeColorToolCallIconErrorForeground   = "tool_call_icon_error_foreground"
	ThemeColorToolCallPathForeground        = "tool_call_path_foreground"
	ThemeColorToolCallAddForeground         = "tool_call_add_foreground"
	ThemeColorToolCallDeleteForeground      = "tool_call_delete_foreground"
	ThemeColorInputBorder                   = "input_border"
	ThemeColorInputPromptDefault            = "input_prompt_default"
	ThemeColorInputPromptShell              = "input_prompt_shell"
	ThemeColorSidebarWarningForeground      = "sidebar_warning_foreground"
	ThemeColorSidebarErrorForeground        = "sidebar_error_foreground"
	ThemeColorSidebarSuccessForeground      = "sidebar_success_foreground"
	ThemeColorSidebarSeparatorForeground    = "sidebar_separator_foreground"
	ThemeColorModelPickerSelectedForeground = "model_picker_selected_foreground"
	ThemeColorModelPickerMutedForeground    = "model_picker_muted_foreground"
	ThemeColorModelPickerBorderForeground   = "model_picker_border_foreground"
)

type Base16Palette struct {
	Base00 color.Color
	Base01 color.Color
	Base02 color.Color
	Base03 color.Color
	Base04 color.Color
	Base05 color.Color
	Base06 color.Color
	Base07 color.Color
	Base08 color.Color
	Base09 color.Color
	Base0A color.Color
	Base0B color.Color
	Base0C color.Color
	Base0D color.Color
	Base0E color.Color
	Base0F color.Color
}

func TerminalBase16Palette() Base16Palette {
	return Base16Palette{
		Base00: lipgloss.Color("0"),
		Base01: lipgloss.Color("1"),
		Base02: lipgloss.Color("2"),
		Base03: lipgloss.Color("3"),
		Base04: lipgloss.Color("4"),
		Base05: lipgloss.Color("5"),
		Base06: lipgloss.Color("6"),
		Base07: lipgloss.Color("7"),
		Base08: lipgloss.Color("8"),
		Base09: lipgloss.Color("9"),
		Base0A: lipgloss.Color("10"),
		Base0B: lipgloss.Color("11"),
		Base0C: lipgloss.Color("12"),
		Base0D: lipgloss.Color("13"),
		Base0E: lipgloss.Color("14"),
		Base0F: lipgloss.Color("15"),
	}
}

type Theme struct {
	palette     Base16Palette
	usageToBase map[string]string
}

func NewTheme(palette Base16Palette) Theme {
	return Theme{
		palette: palette,
		usageToBase: map[string]string{
			ThemeColorShellMessageBackground:        "Base0F",
			ThemeColorStatusSpinnerForeground:       "Base0C",
			ThemeColorStatusDurationForeground:      "Base0C",
			ThemeColorStatusInterruptHintForeground: "Base08",
			ThemeColorUserMessageBorderForeground:   "Base0C",
			ThemeColorThinkingForeground:            "Base08",
			ThemeColorTurnFooterForeground:          "Base07",
			ThemeColorToolCallIconSuccessForeground: "Base02",
			ThemeColorToolCallIconErrorForeground:   "Base01",
			ThemeColorToolCallPathForeground:        "Base0C",
			ThemeColorToolCallAddForeground:         "Base02",
			ThemeColorToolCallDeleteForeground:      "Base01",
			ThemeColorInputBorder:                   "Base08",
			ThemeColorInputPromptDefault:            "Base02",
			ThemeColorInputPromptShell:              "Base0D",
			ThemeColorSidebarWarningForeground:      "Base09",
			ThemeColorSidebarErrorForeground:        "Base01",
			ThemeColorSidebarSuccessForeground:      "Base02",
			ThemeColorSidebarSeparatorForeground:    "Base07",
			ThemeColorModelPickerSelectedForeground: "Base00",
			ThemeColorModelPickerMutedForeground:    "Base08",
			ThemeColorModelPickerBorderForeground:   "Base0E",
		},
	}
}

func DefaultTheme() Theme {
	return NewTheme(TerminalBase16Palette())
}

func (t Theme) Background() color.Color {
	return t.palette.Base00
}

func (t Theme) Surface() color.Color {
	return t.palette.Base01
}

func (t Theme) Foreground() color.Color {
	return t.palette.Base05
}

func (t Theme) Emphasis() color.Color {
	return t.palette.Base06
}

func (t Theme) Muted() color.Color {
	return t.palette.Base03
}

func (t Theme) Error() color.Color {
	return t.palette.Base08
}

func (t Theme) Warning() color.Color {
	return t.palette.Base09
}

func (t Theme) Success() color.Color {
	return t.palette.Base0B
}

func (t Theme) Info() color.Color {
	return t.palette.Base0D
}

func (t Theme) Accent() color.Color {
	return t.palette.Base0E
}

func (t Theme) Color(usage string) color.Color {
	baseName, ok := t.usageToBase[usage]
	if !ok {
		return t.Foreground()
	}

	color, ok := t.colorByBaseName(baseName)
	if !ok {
		return t.Foreground()
	}
	return color
}

func (t Theme) colorByBaseName(baseName string) (color.Color, bool) {
	switch baseName {
	case "Base00":
		return t.palette.Base00, true
	case "Base01":
		return t.palette.Base01, true
	case "Base02":
		return t.palette.Base02, true
	case "Base03":
		return t.palette.Base03, true
	case "Base04":
		return t.palette.Base04, true
	case "Base05":
		return t.palette.Base05, true
	case "Base06":
		return t.palette.Base06, true
	case "Base07":
		return t.palette.Base07, true
	case "Base08":
		return t.palette.Base08, true
	case "Base09":
		return t.palette.Base09, true
	case "Base0A":
		return t.palette.Base0A, true
	case "Base0B":
		return t.palette.Base0B, true
	case "Base0C":
		return t.palette.Base0C, true
	case "Base0D":
		return t.palette.Base0D, true
	case "Base0E":
		return t.palette.Base0E, true
	case "Base0F":
		return t.palette.Base0F, true
	default:
		return nil, false
	}
}
