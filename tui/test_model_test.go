package tui

import (
	"time"

	"charm.land/bubbles/v2/spinner"
	"charm.land/bubbles/v2/stopwatch"
	"github.com/liznear/hh/tui/session"
)

func newTestModel() *model {
	return &model{
		theme:           DefaultTheme(),
		modelName:       "test-model",
		spinner:         spinner.New(spinner.WithSpinner(spinner.Dot)),
		stopwatch:       stopwatch.New(stopwatch.WithInterval(time.Second)),
		State:           newState(session.NewState("test-model"), "test-model", newTextareaInput(), ""),
		markdownCache:   map[string]string{},
		itemRenderCache: map[uintptr]itemRenderCacheEntry{},
	}
}
