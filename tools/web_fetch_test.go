package tools

import (
	"context"
	"encoding/json"
	"net/http"
	"net/http/httptest"
	"testing"
)

func TestWebFetchToolSuccess(t *testing.T) {
	t.Parallel()

	srv := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, _ *http.Request) {
		w.WriteHeader(http.StatusOK)
		_, _ = w.Write([]byte("hello world"))
	}))
	defer srv.Close()

	res := NewWebFetchTool().Handler.Handle(context.Background(), map[string]any{"url": srv.URL})
	if res.IsErr {
		t.Fatalf("expected success, got error: %s", res.Data)
	}

	structured, ok := res.Result.(WebFetchResult)
	if !ok {
		t.Fatalf("unexpected result type: %T", res.Result)
	}
	if structured.StatusCode != http.StatusOK || !structured.OK || structured.Body != "hello world" {
		t.Fatalf("unexpected structured result: %+v", structured)
	}

	var payload WebFetchResult
	if err := json.Unmarshal([]byte(res.Data), &payload); err != nil {
		t.Fatalf("expected json payload, got error: %v", err)
	}
	if payload.StatusCode != http.StatusOK {
		t.Fatalf("unexpected payload status code: %d", payload.StatusCode)
	}
}

func TestWebFetchToolNonSuccessStatus(t *testing.T) {
	t.Parallel()

	srv := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, _ *http.Request) {
		w.WriteHeader(http.StatusNotFound)
		_, _ = w.Write([]byte("missing"))
	}))
	defer srv.Close()

	res := NewWebFetchTool().Handler.Handle(context.Background(), map[string]any{"url": srv.URL})
	if !res.IsErr {
		t.Fatalf("expected error for non-2xx status")
	}

	structured, ok := res.Result.(WebFetchResult)
	if !ok {
		t.Fatalf("unexpected result type: %T", res.Result)
	}
	if structured.StatusCode != http.StatusNotFound || structured.OK {
		t.Fatalf("unexpected structured result: %+v", structured)
	}
}
