package tools

import (
	"bytes"
	"context"
	"encoding/json"
	"fmt"
	"io"
	"net/http"
	"os"
	"strings"

	"github.com/liznear/hh/agent"
)

var exaMCPURL = "https://mcp.exa.ai/mcp"

type WebSearchResult struct {
	Query         string `json:"query"`
	ResponseChars int    `json:"response_chars"`
}

func (r WebSearchResult) Summary() string {
	return fmt.Sprintf("%d chars", r.ResponseChars)
}

type mcpRequest struct {
	JSONRPC string    `json:"jsonrpc"`
	ID      uint64    `json:"id"`
	Method  string    `json:"method"`
	Params  mcpParams `json:"params"`
}

type mcpParams struct {
	Name      string       `json:"name"`
	Arguments mcpArguments `json:"arguments"`
}

type mcpArguments struct {
	Query                string  `json:"query"`
	NumResults           *int    `json:"numResults,omitempty"`
	Livecrawl            *string `json:"livecrawl,omitempty"`
	Type                 *string `json:"type,omitempty"`
	ContextMaxCharacters *int    `json:"contextMaxCharacters,omitempty"`
}

type mcpResponse struct {
	Result *mcpResult `json:"result"`
}

type mcpResult struct {
	Content []mcpContent `json:"content"`
}

type mcpContent struct {
	Type string `json:"type"`
	Text string `json:"text"`
}

func NewWebSearchTool() agent.Tool {
	return agent.Tool{
		Name:        "web_search",
		Description: "Search the web for information. Returns search results with titles, snippets, and URLs.",
		Schema: map[string]any{
			"type": "object",
			"properties": map[string]any{
				"query": map[string]any{
					"type":        "string",
					"description": "The search query",
				},
			},
			"required": []string{"query"},
		},
		Handler: agent.FuncToolHandler(handleWebSearch),
	}
}

func handleWebSearch(ctx context.Context, params map[string]any) agent.ToolResult {
	query, err := requiredString(params, "query")
	if err != nil {
		return toolErr("%s", err.Error())
	}

	if query == "" {
		return toolErr("invalid_input: query is required")
	}

	numResults := 8
	livecrawl := "fallback"
	typeAuto := "auto"
	contextMaxCharacters := 10000

	reqPayload := mcpRequest{
		JSONRPC: "2.0",
		ID:      1,
		Method:  "tools/call",
		Params: mcpParams{
			Name: "web_search_exa",
			Arguments: mcpArguments{
				Query:                query,
				NumResults:           &numResults,
				Livecrawl:            &livecrawl,
				Type:                 &typeAuto,
				ContextMaxCharacters: &contextMaxCharacters,
			},
		},
	}

	body, err := json.Marshal(reqPayload)
	if err != nil {
		return toolErr("request_error: failed to encode request: %v", err)
	}

	req, err := http.NewRequestWithContext(ctx, http.MethodPost, exaMCPURL, bytes.NewReader(body))
	if err != nil {
		return toolErr("request_error: search request failed: %v", err)
	}
	req.Header.Set("User-Agent", defaultUserAgent)
	req.Header.Set("Content-Type", "application/json")
	req.Header.Set("Accept", "application/json, text/event-stream")

	if apiKey := os.Getenv("HH_EXA_API_KEY"); apiKey != "" {
		req.Header.Set("x-api-key", apiKey)
	}

	resp, err := webHTTPClient.Do(req)
	if err != nil {
		return toolErr("request_error: search request failed: %v", err)
	}
	defer resp.Body.Close()

	responseBody, err := io.ReadAll(resp.Body)
	if err != nil {
		return toolErr("read_body_error: failed to read response: %v", err)
	}

	if resp.StatusCode < 200 || resp.StatusCode >= 300 {
		return toolErr("search_failed: search failed: status=%s, body=%s", resp.Status, string(responseBody))
	}

	responseText := string(responseBody)
	for _, line := range strings.Split(responseText, "\n") {
		jsonStr, ok := strings.CutPrefix(line, "data: ")
		if !ok {
			continue
		}

		var parsed mcpResponse
		if err := json.Unmarshal([]byte(jsonStr), &parsed); err != nil {
			return toolErr("parse_error: failed to parse MCP response: %v", err)
		}

		if parsed.Result != nil && len(parsed.Result.Content) > 0 {
			text := parsed.Result.Content[0].Text
			return agent.ToolResult{
				Data: text,
				Result: WebSearchResult{
					Query:         query,
					ResponseChars: len(text),
				},
			}
		}
	}

	return toolErr("no_results: No search results found. Please try a different query.")
}
