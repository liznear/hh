package tools

import (
	"context"
	"strings"
	"testing"
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
