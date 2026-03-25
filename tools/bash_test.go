package tools

import (
	"context"
	"strings"
	"testing"
	"time"

	"github.com/liznear/hh/agent"
)

func TestBashTool(t *testing.T) {
	res := NewBashTool().Handler.Handle(context.Background(), map[string]any{"command": "printf 'hello'"})
	if res.IsErr {
		t.Fatalf("expected success, got error: %s", res.Data)
	}
	if res.Data != "hello" {
		t.Fatalf("unexpected output: %q", res.Data)
	}

	structured, ok := res.Result.(BashResult)
	if !ok {
		t.Fatalf("unexpected result type: %T", res.Result)
	}
	if structured.Command != "printf 'hello'" || structured.ExitCode != 0 {
		t.Fatalf("unexpected structured result: %+v", structured)
	}
}

func TestBashTool_NonZeroExit(t *testing.T) {
	res := NewBashTool().Handler.Handle(context.Background(), map[string]any{"command": `echo "bad" 1>&2; exit 7`})
	if !res.IsErr {
		t.Fatalf("expected error for non-zero exit")
	}
	if !strings.Contains(res.Data, "bad") {
		t.Fatalf("unexpected output: %q", res.Data)
	}

	structured, ok := res.Result.(BashResult)
	if !ok {
		t.Fatalf("unexpected result type: %T", res.Result)
	}
	if structured.ExitCode != 7 {
		t.Fatalf("unexpected exit code: %d", structured.ExitCode)
	}
}

func TestBashTool_Cancellation(t *testing.T) {
	ctx, cancel := context.WithCancel(context.Background())

	// Start a long-running command
	done := make(chan agent.ToolResult, 1)
	go func() {
		res := NewBashTool().Handler.Handle(ctx, map[string]any{"command": "sleep 10"})
		done <- res
	}()

	// Cancel after a short delay
	time.Sleep(100 * time.Millisecond)
	cancel()

	// Should get a quick response after cancellation
	select {
	case res := <-done:
		if !res.IsErr {
			t.Fatalf("expected error after cancellation, got success")
		}
		if !strings.Contains(res.Data, "interrupted") {
			t.Fatalf("expected interrupted message, got: %q", res.Data)
		}
	case <-time.After(2 * time.Second):
		t.Fatal("bash command should have been interrupted quickly")
	}
}
