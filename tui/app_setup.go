package tui

import (
	"fmt"
	"os"

	"charm.land/bubbles/v2/textarea"
	"github.com/charmbracelet/lipgloss"
	"github.com/liznear/hh/tui/session"
)

func newTextareaInput() textarea.Model {
	in := textarea.New()
	in.Prompt = ""
	in.SetPromptFunc(2, func(info textarea.PromptInfo) string {
		if info.LineNumber == 0 {
			return "  > "
		}
		return " :: "
	})
	in.Placeholder = ""
	in.ShowLineNumbers = false
	in.SetHeight(inputInnerLines)
	applyTextareaPromptColor(&in, DefaultTheme().Color(ThemeColorInputPromptDefault))
	in.Focus()

	return in
}

func applyTextareaPromptColor(in *textarea.Model, promptColor lipgloss.Color) {
	styles := textarea.DefaultStyles(false)
	styles.Focused.Base = styles.Focused.Base.UnsetBackground()
	styles.Focused.Text = styles.Focused.Text.UnsetBackground()
	styles.Focused.CursorLine = styles.Focused.CursorLine.UnsetBackground()
	styles.Focused.Placeholder = styles.Focused.Placeholder.UnsetBackground()
	styles.Focused.Prompt = styles.Focused.Prompt.
		UnsetBackground().
		Foreground(promptColor).
		Bold(true)
	styles.Focused.EndOfBuffer = styles.Focused.EndOfBuffer.UnsetBackground()
	styles.Blurred.Base = styles.Blurred.Base.UnsetBackground()
	styles.Blurred.Text = styles.Blurred.Text.UnsetBackground()
	styles.Blurred.CursorLine = styles.Blurred.CursorLine.UnsetBackground()
	styles.Blurred.Placeholder = styles.Blurred.Placeholder.UnsetBackground()
	styles.Blurred.Prompt = styles.Blurred.Prompt.
		UnsetBackground().
		Foreground(promptColor).
		Bold(true)
	styles.Blurred.EndOfBuffer = styles.Blurred.EndOfBuffer.UnsetBackground()
	in.SetStyles(styles)
}

func newSessionStorage(state *session.State) *session.Storage {
	if state == nil {
		return nil
	}
	dir, err := session.DefaultStorageDir()
	if err != nil {
		fmt.Fprintf(os.Stderr, "failed to resolve session storage directory: %v\n", err)
		return nil
	}

	store, err := session.NewStorage(dir)
	if err != nil {
		fmt.Fprintf(os.Stderr, "failed to initialize session storage: %v\n", err)
		return nil
	}

	return store
}
