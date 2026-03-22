package tools

import (
	"context"
	"encoding/json"
	"fmt"
	"io"
	"net/http"

	"github.com/liznear/hh/agent"
)

const defaultUserAgent = "Mozilla/5.0 (compatible; hh-agent/1.0)"

var webHTTPClient = &http.Client{}

type WebFetchResult struct {
	URL        string `json:"url"`
	StatusCode int    `json:"status_code"`
	OK         bool   `json:"ok"`
	Body       string `json:"body"`
}

func (r WebFetchResult) Summary() string {
	return fmt.Sprintf("status %d", r.StatusCode)
}

func NewWebFetchTool() agent.Tool {
	return agent.Tool{
		Name:        "web_fetch",
		Description: "Fetch content from a URL",
		Schema: map[string]any{
			"type": "object",
			"properties": map[string]any{
				"url": map[string]any{"type": "string"},
			},
			"required": []string{"url"},
		},
		Handler: agent.FuncToolHandler(handleWebFetch),
	}
}

func handleWebFetch(ctx context.Context, params map[string]any) agent.ToolResult {
	url, err := requiredString(params, "url")
	if err != nil {
		return toolErr("%s", err.Error())
	}

	req, err := http.NewRequestWithContext(ctx, http.MethodGet, url, nil)
	if err != nil {
		return toolErr("request_error: %v", err)
	}
	req.Header.Set("User-Agent", defaultUserAgent)

	resp, err := webHTTPClient.Do(req)
	if err != nil {
		return toolErr("request_error: %v", err)
	}
	defer resp.Body.Close()

	body, err := io.ReadAll(resp.Body)
	if err != nil {
		return toolErr("read_body_error: %v", err)
	}

	result := WebFetchResult{
		URL:        url,
		StatusCode: resp.StatusCode,
		OK:         resp.StatusCode >= 200 && resp.StatusCode < 300,
		Body:       string(body),
	}

	payload, err := json.Marshal(result)
	if err != nil {
		payload = []byte(fmt.Sprintf(`{"status_code":%d}`, resp.StatusCode))
	}

	out := agent.ToolResult{
		Result:      result,
		Data:        string(payload),
		ContentType: "application/json",
	}

	if !result.OK {
		out.IsErr = true
	}

	return out
}
