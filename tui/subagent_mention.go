package tui

import (
	"context"
	"encoding/json"
	"fmt"
	"strings"
	"time"

	tea "charm.land/bubbletea/v2"
	"github.com/liznear/hh/agent"
	"github.com/liznear/hh/config"
	"github.com/liznear/hh/skills"
	"github.com/liznear/hh/tools"
	"github.com/liznear/hh/tui/agents"
)

var startMentionSubAgentStreamCmdWithContext = startMentionSubAgentStreamCmdWithContextImpl

func parseSubAgentInvocation(prompt string) (subAgentName string, taskPrompt string, ok bool) {
	trimmed := strings.TrimSpace(prompt)
	if !strings.HasPrefix(trimmed, "@") {
		return "", "", false
	}

	tokenBody := strings.TrimSpace(strings.TrimPrefix(trimmed, "@"))
	if tokenBody == "" {
		return "", "", false
	}

	token := tokenBody
	rest := ""
	if idx := strings.IndexAny(tokenBody, " \t\n\r"); idx >= 0 {
		token = strings.TrimSpace(tokenBody[:idx])
		rest = strings.TrimSpace(tokenBody[idx:])
	}
	if token == "" {
		return "", "", false
	}
	if strings.ContainsAny(token, "/\\") {
		return "", "", false
	}

	canonical, found := resolveSubAgentCanonicalName(token)
	if !found {
		return "", "", false
	}
	return canonical, rest, true
}

var resolveSubAgentCanonicalName = func(name string) (string, bool) {
	catalog, err := agents.LoadDefaultCatalog()
	if err != nil {
		return "", false
	}
	needle := strings.TrimSpace(name)
	if needle == "" {
		return "", false
	}
	for _, sub := range catalog.SubAgents() {
		candidate := strings.TrimSpace(sub.Name)
		if candidate == "" {
			continue
		}
		if strings.EqualFold(candidate, needle) {
			return candidate, true
		}
	}
	return "", false
}

func mentionTaskLabel(subAgentName, taskPrompt, effectivePrompt string) string {
	trimmed := strings.TrimSpace(taskPrompt)
	if trimmed != "" {
		return trimmed
	}
	if strings.EqualFold(strings.TrimSpace(subAgentName), "Reviewer") {
		return "Review uncommitted changes"
	}
	if strings.TrimSpace(effectivePrompt) != "" {
		return strings.TrimSpace(effectivePrompt)
	}
	return "(empty prompt)"
}

func effectiveSubAgentPrompt(subAgentName, taskPrompt string) string {
	trimmed := strings.TrimSpace(taskPrompt)
	if trimmed != "" {
		return trimmed
	}
	if strings.EqualFold(strings.TrimSpace(subAgentName), "Reviewer") {
		return "Review all uncommitted changes in this repository. Start with `git status --short` and `git diff --` (and `git diff --cached` if needed). Report findings with severity, file references, and suggested fixes."
	}
	return ""
}

func startMentionSubAgentStreamCmdWithContextImpl(ctx context.Context, cfg config.Config, modelName, workingDir, subAgentName, taskPrompt, internalState, toolCallID string) tea.Cmd {
	return func() tea.Msg {
		ch := make(chan tea.Msg)
		go func() {
			err := runMentionSubAgentTask(ctx, cfg, modelName, workingDir, subAgentName, taskPrompt, internalState, toolCallID, func(e agent.Event) {
				ch <- agentEventMsg{event: e}
			})
			ch <- agentRunDoneMsg{err: err}
			close(ch)
		}()
		return agentStreamStartedMsg{ch: ch}
	}
}

func runMentionSubAgentTask(ctx context.Context, cfg config.Config, modelName, workingDir, subAgentName, taskPrompt, internalState, toolCallID string, emit func(agent.Event)) error {
	effectivePrompt := effectiveSubAgentPrompt(subAgentName, taskPrompt)
	taskLabel := mentionTaskLabel(subAgentName, taskPrompt, effectivePrompt)
	toolCall := buildMentionTaskToolCall(toolCallID, subAgentName, taskLabel)
	emit(agent.Event{Type: agent.EventTypeToolCallStart, ToolCallID: toolCall.ID, Data: agent.EventDataToolCallStart{Call: toolCall}})

	result := tools.TaskTaskResult{SubAgentName: subAgentName, Task: taskLabel}

	catalog, err := agents.LoadDefaultCatalog()
	if err != nil {
		result.Status = tools.TaskTaskStatusError
		result.Error = fmt.Sprintf("load agent catalog: %v", err)
		emitMentionTaskToolEnd(toolCall, result, emit)
		return nil
	}

	agentConfig, ok := catalog.Get(subAgentName)
	if !ok || agentConfig.Type != agents.AgentTypeSubAgent {
		result.Status = tools.TaskTaskStatusError
		result.Error = fmt.Sprintf("sub-agent %q not found", subAgentName)
		emitMentionTaskToolEnd(toolCall, result, emit)
		return nil
	}

	provider, err := cfg.ModelRouterProvider()
	if err != nil {
		result.Status = tools.TaskTaskStatusError
		result.Error = fmt.Sprintf("resolve provider: %v", err)
		emitMentionTaskToolEnd(toolCall, result, emit)
		return nil
	}

	approver, err := newToolApprover(cfg, workingDir)
	if err != nil {
		result.Status = tools.TaskTaskStatusError
		result.Error = fmt.Sprintf("build approver: %v", err)
		emitMentionTaskToolEnd(toolCall, result, emit)
		return nil
	}

	runID := fmt.Sprintf("mention_sub_agent_%d", time.Now().UnixNano())
	aCtx := agent.Context{
		Model:        modelName,
		Provider:     provider,
		SystemPrompt: buildMentionSubAgentSystemPrompt(agentConfig.SystemPrompt, workingDir),
		History:      nil,
		Prompts:      nil,
		Tools:        tools.GetTools(agentConfig.AllowedTools),
		Approver:     approver,
		RunID:        runID,
		Interactions: agent.NewInteractionManager(),
		Steering:     agent.NewSteeringQueue(),
	}

	if strings.TrimSpace(effectivePrompt) != "" {
		promptMessage := agent.Message{Role: agent.RoleUser, Content: strings.TrimSpace(effectivePrompt), InternalState: internalState}
		aCtx.Prompts = append(aCtx.Prompts, promptMessage)
		emitMentionTaskProgress(toolCall.ID, 0, subAgentName, taskLabel, agent.Event{Type: agent.EventTypeMessage, Data: agent.EventDataMessage{Message: promptMessage}}, emit)
	}

	var (
		messages []agent.Message
		runErr   error
	)
	agent.RunAgentLoop(ctx, aCtx, func(subEvent agent.Event) {
		switch subEvent.Type {
		case agent.EventTypeAgentEnd:
			if data, ok := subEvent.Data.(agent.EventDataAgentEnd); ok {
				messages = data.Messages
			}
			emitMentionTaskProgress(toolCall.ID, 0, subAgentName, taskLabel, subEvent, emit)
		case agent.EventTypeMessage, agent.EventTypeToolCallStart, agent.EventTypeToolCallEnd, agent.EventTypeMessageDelta, agent.EventTypeThinkingDelta:
			emitMentionTaskProgress(toolCall.ID, 0, subAgentName, taskLabel, subEvent, emit)
		case agent.EventTypeError:
			switch data := subEvent.Data.(type) {
			case error:
				runErr = data
			case agent.EventDataError:
				runErr = data.Err
			}
			emitMentionTaskProgress(toolCall.ID, 0, subAgentName, taskLabel, subEvent, emit)
		}
	})

	if len(messages) > 0 {
		result.Messages = append([]agent.Message(nil), messages...)
	}
	if runErr != nil {
		result.Status = tools.TaskTaskStatusError
		result.Error = fmt.Sprintf("run sub-agent %q: %v", subAgentName, runErr)
	} else {
		result.Status = tools.TaskTaskStatusSuccess
		result.Output = extractFinalAssistantOutput(messages)
	}

	emitMentionTaskToolEnd(toolCall, result, emit)
	return nil
}

func buildMentionTaskToolCall(toolCallID, subAgentName, taskLabel string) agent.ToolCall {
	if strings.TrimSpace(toolCallID) == "" {
		toolCallID = fmt.Sprintf("mention_task_%d", time.Now().UnixNano())
	}
	arguments := map[string]any{
		"tasks": []map[string]any{{
			"sub_agent_name": subAgentName,
			"task":           taskLabel,
		}},
	}
	argsBytes, _ := json.Marshal(arguments)
	return agent.ToolCall{ID: toolCallID, Name: "task", Arguments: string(argsBytes)}
}

func emitMentionTaskProgress(parentToolCallID string, taskIndex int, subAgentName, task string, subEvent agent.Event, emit func(agent.Event)) {
	emit(agent.Event{
		Type:       agent.EventTypeTaskProgress,
		ToolCallID: parentToolCallID,
		Data: agent.EventDataTaskProgress{
			ParentToolCallID: parentToolCallID,
			TaskIndex:        taskIndex,
			SubAgentName:     strings.TrimSpace(subAgentName),
			Task:             strings.TrimSpace(task),
			SubEvent:         subEvent,
		},
	})
}

func emitMentionTaskToolEnd(call agent.ToolCall, taskResult tools.TaskTaskResult, emit func(agent.Event)) {
	payload := tools.TaskResult{Tasks: []tools.TaskTaskResult{taskResult}}
	data := taskResult.Output
	if taskResult.Status == tools.TaskTaskStatusError {
		data = taskResult.Error
	}
	emit(agent.Event{
		Type:       agent.EventTypeToolCallEnd,
		ToolCallID: call.ID,
		Data:       agent.EventDataToolCallEnd{Call: call, Result: agent.ToolResult{Data: data, Result: payload}},
	})
}

func extractFinalAssistantOutput(messages []agent.Message) string {
	for i := len(messages) - 1; i >= 0; i-- {
		if messages[i].Role != agent.RoleAssistant {
			continue
		}
		content := strings.TrimSpace(messages[i].Content)
		if content == "" {
			continue
		}
		return content
	}
	return ""
}

func buildMentionSubAgentSystemPrompt(base string, workingDir string) string {
	parts := make([]string, 0, 4)
	if trimmed := strings.TrimSpace(base); trimmed != "" {
		parts = append(parts, trimmed)
	}

	skillCatalog, err := skills.LoadDefaultCatalog()
	if err == nil {
		skillBlock := strings.TrimSpace(skillCatalog.PromptFrontmatterBlock())
		if skillBlock != "" {
			parts = append(parts, skillBlock)
		}
	}

	if globalAgentsMD := readAgentsMDFile(getGlobalAgentsMDPath()); globalAgentsMD != "" {
		parts = append(parts, "<global-agents-md>\n"+globalAgentsMD+"\n</global-agents-md>")
	}
	if projectAgentsMD := readAgentsMDFile(projectAgentsMDPath(workingDir)); projectAgentsMD != "" {
		parts = append(parts, "<project-agents-md>\n"+projectAgentsMD+"\n</project-agents-md>")
	}

	return strings.Join(parts, "\n\n")
}
