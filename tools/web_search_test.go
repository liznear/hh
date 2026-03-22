package tools

import (
	"context"
	"encoding/json"
	"net/http"
	"net/http/httptest"
	"testing"
)

func TestWebSearchToolSuccess(t *testing.T) {
	oldURL := exaMCPURL
	t.Cleanup(func() { exaMCPURL = oldURL })

	srv := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		if r.Method != http.MethodPost {
			t.Fatalf("unexpected method: %s", r.Method)
		}
		var req mcpRequest
		if err := json.NewDecoder(r.Body).Decode(&req); err != nil {
			t.Fatalf("failed to decode request: %v", err)
		}
		if req.Params.Arguments.Query != "golang" {
			t.Fatalf("unexpected query: %q", req.Params.Arguments.Query)
		}

		w.WriteHeader(http.StatusOK)
		_, _ = w.Write([]byte("data: {\"result\":{\"content\":[{\"type\":\"text\",\"text\":\"answer\"}]}}\n"))
	}))
	defer srv.Close()
	exaMCPURL = srv.URL

	res := NewWebSearchTool().Handler.Handle(context.Background(), map[string]any{"query": "golang"})
	if res.IsErr {
		t.Fatalf("expected success, got error: %s", res.Data)
	}
	if res.Data != "answer" {
		t.Fatalf("unexpected body: %q", res.Data)
	}

	structured, ok := res.Result.(WebSearchResult)
	if !ok {
		t.Fatalf("unexpected result type: %T", res.Result)
	}
	if structured.Query != "golang" || structured.ResponseChars != len("answer") {
		t.Fatalf("unexpected structured result: %+v", structured)
	}
}

func TestWebSearchToolParseError(t *testing.T) {
	oldURL := exaMCPURL
	t.Cleanup(func() { exaMCPURL = oldURL })

	srv := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, _ *http.Request) {
		w.WriteHeader(http.StatusOK)
		_, _ = w.Write([]byte("data: not-json\n"))
	}))
	defer srv.Close()
	exaMCPURL = srv.URL

	res := NewWebSearchTool().Handler.Handle(context.Background(), map[string]any{"query": "golang"})
	if !res.IsErr {
		t.Fatalf("expected parse error")
	}
}
