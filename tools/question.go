package tools

import (
	"context"
	"encoding/json"
	"fmt"
	"strings"
	"time"

	"github.com/liznear/hh/agent"
)

type questionInput struct {
	Question          questionPrompt   `json:"question"`
	Options           []questionOption `json:"options"`
	AllowCustomOption bool             `json:"allow_custom_option"`
}

type questionPrompt struct {
	Title       string `json:"title"`
	Content     string `json:"content,omitempty"`
	ContentType string `json:"content_type,omitempty"`
}

type questionOption struct {
	Title       string `json:"title"`
	Description string `json:"description"`
}

type QuestionResult struct {
	Type   string                `json:"type"`
	Option *QuestionResultOption `json:"option,omitempty"`
	Custom string                `json:"custom,omitempty"`
}

type QuestionResultOption struct {
	Index       int    `json:"index"`
	Title       string `json:"title"`
	Description string `json:"description"`
}

func NewQuestionTool() agent.Tool {
	return agent.Tool{
		Name:        "question",
		Description: "Ask user for choice or custom input",
		Schema: map[string]any{
			"type": "object",
			"properties": map[string]any{
				"question": map[string]any{
					"type": "object",
					"properties": map[string]any{
						"title":        map[string]any{"type": "string"},
						"content":      map[string]any{"type": "string"},
						"content_type": map[string]any{"type": "string"},
					},
					"required": []string{"title"},
				},
				"options": map[string]any{
					"type": "array",
					"items": map[string]any{
						"type": "object",
						"properties": map[string]any{
							"title":       map[string]any{"type": "string"},
							"description": map[string]any{"type": "string"},
						},
						"required": []string{"title", "description"},
					},
				},
				"allow_custom_option": map[string]any{"type": "boolean"},
			},
			"required": []string{"question", "options", "allow_custom_option"},
		},
		Handler: agent.FuncToolHandler(handleQuestion),
	}
}

func handleQuestion(ctx context.Context, params map[string]any) agent.ToolResult {
	input, err := parseQuestionInput(params)
	if err != nil {
		return toolErr("%s", err.Error())
	}

	request := agent.InteractionRequest{
		InteractionID:     fmt.Sprintf("question_%s_%d", sanitizeInteractionSuffix(input.Question.Title), time.Now().UnixNano()),
		Kind:              agent.InteractionKindQuestion,
		Title:             input.Question.Title,
		Content:           input.Question.Content,
		ContentType:       input.Question.ContentType,
		Options:           make([]agent.InteractionOption, 0, len(input.Options)),
		AllowCustomOption: input.AllowCustomOption,
	}
	for idx, opt := range input.Options {
		request.Options = append(request.Options, agent.InteractionOption{
			ID:          fmt.Sprintf("option_%d", idx+1),
			Title:       opt.Title,
			Description: opt.Description,
		})
	}

	response, err := agent.RequestInteraction(ctx, request)
	if err != nil {
		return toolErr("question failed: %v", err)
	}

	result, err := mapQuestionResult(input, request, response)
	if err != nil {
		return toolErr("question failed: %v", err)
	}

	body, err := json.Marshal(result)
	if err != nil {
		return toolErr("question failed: %v", err)
	}

	return agent.ToolResult{Data: string(body), Result: result}
}

func parseQuestionInput(params map[string]any) (questionInput, error) {
	raw, err := json.Marshal(params)
	if err != nil {
		return questionInput{}, fmt.Errorf("invalid question parameters")
	}

	var input questionInput
	if err := json.Unmarshal(raw, &input); err != nil {
		return questionInput{}, fmt.Errorf("invalid question parameters")
	}

	input.Question.Title = strings.TrimSpace(input.Question.Title)
	if input.Question.Title == "" {
		return questionInput{}, fmt.Errorf("question.title must be a non-empty string")
	}
	if len(input.Options) == 0 {
		return questionInput{}, fmt.Errorf("options must contain at least one option")
	}
	for idx, opt := range input.Options {
		if strings.TrimSpace(opt.Title) == "" {
			return questionInput{}, fmt.Errorf("options[%d].title must be a non-empty string", idx)
		}
		if strings.TrimSpace(opt.Description) == "" {
			return questionInput{}, fmt.Errorf("options[%d].description must be a non-empty string", idx)
		}
	}

	return input, nil
}

func mapQuestionResult(input questionInput, req agent.InteractionRequest, resp agent.InteractionResponse) (QuestionResult, error) {
	if resp.CustomText != "" {
		custom := strings.TrimSpace(resp.CustomText)
		if custom == "" {
			return QuestionResult{}, fmt.Errorf("custom answer must be non-empty")
		}
		if !input.AllowCustomOption {
			return QuestionResult{}, fmt.Errorf("custom answer not allowed")
		}
		return QuestionResult{Type: "custom", Custom: custom}, nil
	}

	for idx, option := range req.Options {
		if option.ID == resp.SelectedOptionID {
			return QuestionResult{
				Type: "option",
				Option: &QuestionResultOption{
					Index:       idx + 1,
					Title:       option.Title,
					Description: option.Description,
				},
			}, nil
		}
	}

	return QuestionResult{}, fmt.Errorf("selected option not found")
}

func sanitizeInteractionSuffix(title string) string {
	title = strings.ToLower(strings.TrimSpace(title))
	if title == "" {
		return "prompt"
	}
	var b strings.Builder
	for _, r := range title {
		switch {
		case (r >= 'a' && r <= 'z') || (r >= '0' && r <= '9'):
			b.WriteRune(r)
		case r == ' ' || r == '-' || r == '_':
			b.WriteRune('_')
		}
		if b.Len() >= 32 {
			break
		}
	}
	if b.Len() == 0 {
		return "prompt"
	}
	return b.String()
}
