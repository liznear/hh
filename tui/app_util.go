package tui

import (
	"fmt"
	"os"
	"strconv"
	"strings"
	"time"

	"github.com/liznear/hh/agent"
)

func ternary[T any](cond bool, ifTrue, ifFalse T) T {
	if cond {
		return ifTrue
	}
	return ifFalse
}

func maxDuration(a, b time.Duration) time.Duration {
	if a > b {
		return a
	}
	return b
}

func maxInt(a, b int) int {
	if a > b {
		return a
	}
	return b
}

func min(a, b int) int {
	if a < b {
		return a
	}
	return b
}

func isDebugEnabled() bool {
	v := strings.TrimSpace(os.Getenv("HH_DEBUG"))
	enabled, err := strconv.ParseBool(v)
	return err == nil && enabled
}

func formatDuration(d time.Duration) string {
	if d >= time.Millisecond {
		return fmt.Sprintf("%.2fms", float64(d)/float64(time.Millisecond))
	}
	return fmt.Sprintf("%dus", d.Microseconds())
}

func toolCallKey(call agent.ToolCall) string {
	if call.ID != "" {
		return call.ID
	}
	return call.Name + "|" + call.Arguments
}

const renderRefreshInterval = 33 * time.Millisecond
const scrollPriorityWindow = 120 * time.Millisecond
const streamBatchMaxEvents = 64
