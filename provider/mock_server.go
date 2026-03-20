package provider

import (
	"bufio"
	"encoding/json"
	"fmt"
	"net/http"
	"net/http/httptest"
	"os"
	"path/filepath"
	"testing"
	"time"

	"github.com/liznear/hh/agent"
)

type mockSessionServer struct {
	t            *testing.T
	sessionName  string
	providerName string
	step         int
}

func (m *mockSessionServer) ServeHTTP(w http.ResponseWriter, r *http.Request) {
	m.step++

	reqFile := filepath.Join("testdata", "sessions", m.sessionName, fmt.Sprintf("req-%d.json", m.step))
	if _, err := os.Stat(reqFile); os.IsNotExist(err) {
		m.t.Errorf("Unexpected request #%d: expected request file %s does not exist", m.step, reqFile)
		http.Error(w, "unexpected request", http.StatusBadRequest)
		return
	}

	respFile := filepath.Join("testdata", "sessions", m.sessionName, fmt.Sprintf("%s-resp-%d.jsonl", m.providerName, m.step))
	if _, err := os.Stat(respFile); os.IsNotExist(err) {
		m.t.Errorf("Response file %s does not exist for step %d", respFile, m.step)
		http.Error(w, "missing response file", http.StatusInternalServerError)
		return
	}

	w.Header().Set("Content-Type", "text/event-stream")
	w.WriteHeader(http.StatusOK)

	f, err := os.Open(respFile)
	if err != nil {
		m.t.Errorf("Failed to open %s: %v", respFile, err)
		return
	}
	defer f.Close()

	scanner := bufio.NewScanner(f)
	for scanner.Scan() {
		line := scanner.Text()
		if line == "" {
			continue
		}

		fmt.Fprintf(w, "data: %s\n\n", line)
		if flusher, ok := w.(http.Flusher); ok {
			flusher.Flush()
		}

		time.Sleep(5 * time.Millisecond)
	}

	fmt.Fprintf(w, "data: [DONE]\n\n")
	if flusher, ok := w.(http.Flusher); ok {
		flusher.Flush()
	}

	if err := scanner.Err(); err != nil {
		m.t.Errorf("Error reading %s: %v", respFile, err)
	}
}

func startMockSessionServer(t *testing.T, sessionName, providerName string) *httptest.Server {
	return httptest.NewServer(&mockSessionServer{
		t:            t,
		sessionName:  sessionName,
		providerName: providerName,
		step:         0,
	})
}

func loadProviderRequest(t *testing.T, sessionName string, step int) agent.ProviderRequest {
	reqFile := filepath.Join("testdata", "sessions", sessionName, fmt.Sprintf("req-%d.json", step))
	b, err := os.ReadFile(reqFile)
	if err != nil {
		t.Fatalf("failed to read %s: %v", reqFile, err)
	}
	var req agent.ProviderRequest
	if err := json.Unmarshal(b, &req); err != nil {
		t.Fatalf("failed to unmarshal %s: %v", reqFile, err)
	}
	return req
}

func loadExpectedEvents(t *testing.T, sessionName string, step int) []agent.ProviderResponse {
	filename := filepath.Join("testdata", "sessions", sessionName, fmt.Sprintf("want-events-%d.jsonl", step))
	f, err := os.Open(filename)
	if err != nil {
		t.Fatalf("failed to open %s: %v", filename, err)
	}
	defer f.Close()

	var events []agent.ProviderResponse
	scanner := bufio.NewScanner(f)
	for scanner.Scan() {
		line := scanner.Text()
		if line == "" {
			continue
		}
		var e agent.ProviderResponse
		if err := json.Unmarshal([]byte(line), &e); err != nil {
			t.Fatalf("failed to unmarshal event %s: %v", line, err)
		}
		events = append(events, e)
	}
	if err := scanner.Err(); err != nil {
		t.Fatalf("error reading events %s: %v", filename, err)
	}
	return events
}
