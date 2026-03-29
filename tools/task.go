package tools

import (
	"context"
	"fmt"
	"os"
	"path/filepath"
	"strings"
	"sync"

	"github.com/liznear/hh/agent"
	"github.com/liznear/hh/tui/agents"
)

type TaskResult struct {
	Tasks []TaskTaskResult `json:"tasks"`
}

type TaskTaskStatus string

const (
	TaskTaskStatusSuccess TaskTaskStatus = "success"
	TaskTaskStatusError   TaskTaskStatus = "error"
)

type TaskTaskResult struct {
	SubAgentName string          `json:"sub_agent_name"`
	Task         string          `json:"task"`
	Status       TaskTaskStatus  `json:"status"`
	Output       string          `json:"output,omitempty"`
	Error        string          `json:"error,omitempty"`
	Messages     []agent.Message `json:"messages,omitempty"`
}

func (r TaskResult) Summary() string {
	if len(r.Tasks) == 0 {
		return "task completed"
	}
	if len(r.Tasks) == 1 {
		return fmt.Sprintf("task completed by %q", r.Tasks[0].SubAgentName)
	}
	return fmt.Sprintf("%d tasks completed", len(r.Tasks))
}

type taskRequest struct {
	SubAgentName string
	Task         string
}

func NewTaskTool() agent.Tool {
	return agent.Tool{
		Name:        "task",
		Description: "Start one or more sub-agents with task descriptions",
		Schema:      taskToolSchema(),
		Handler:     agent.FuncToolHandler(handleTask),
	}
}

func taskToolSchema() map[string]any {
	subAgentProperty := map[string]any{"type": "string"}
	names := availableSubAgentNames()
	if len(names) > 0 {
		subAgentProperty["enum"] = names
		subAgentProperty["description"] = fmt.Sprintf("Registered sub-agent name. Available: %s", strings.Join(names, ", "))
	}

	return map[string]any{
		"type": "object",
		"properties": map[string]any{
			"tasks": map[string]any{
				"type": "array",
				"items": map[string]any{
					"type": "object",
					"properties": map[string]any{
						"sub_agent_name": subAgentProperty,
						"task":           map[string]any{"type": "string"},
					},
					"required": []string{"sub_agent_name", "task"},
				},
			},
		},
		"required": []string{"tasks"},
	}
}

func availableSubAgentNames() []string {
	catalog, err := agents.LoadDefaultCatalog()
	if err != nil {
		return nil
	}
	subAgents := catalog.SubAgents()
	ret := make([]string, 0, len(subAgents))
	for _, entry := range subAgents {
		if strings.TrimSpace(entry.Name) == "" {
			continue
		}
		ret = append(ret, entry.Name)
	}
	return ret
}

func handleTask(ctx context.Context, params map[string]any) agent.ToolResult {
	runtime, ok := agent.ToolRuntimeFromContext(ctx)
	if !ok {
		return toolErr("task tool requires an active agent runtime")
	}
	if runtime.Provider == nil {
		return toolErr("task tool requires an active provider")
	}
	if strings.TrimSpace(runtime.Model) == "" {
		return toolErr("task tool requires an active model")
	}

	requests, err := parseTaskRequests(params)
	if err != nil {
		return toolErr("%s", err.Error())
	}

	results := runTaskRequests(ctx, runtime, requests)

	payload := TaskResult{Tasks: results}
	return agent.ToolResult{
		Data:   formatTaskResultData(payload),
		Result: payload,
	}
}

func parseTaskRequests(params map[string]any) ([]taskRequest, error) {
	rawTasks, ok := params["tasks"]
	if !ok || rawTasks == nil {
		return nil, fmt.Errorf("tasks is required")
	}

	items, ok := rawTasks.([]any)
	if !ok {
		return nil, fmt.Errorf("tasks must be an array")
	}
	if len(items) == 0 {
		return nil, fmt.Errorf("tasks must contain at least one item")
	}

	requests := make([]taskRequest, 0, len(items))
	for idx, raw := range items {
		item, ok := raw.(map[string]any)
		if !ok {
			return nil, fmt.Errorf("tasks[%d] must be an object", idx)
		}
		req, err := parseTaskRequest(item)
		if err != nil {
			return nil, fmt.Errorf("tasks[%d]: %w", idx, err)
		}
		requests = append(requests, req)
	}

	return requests, nil
}

func parseTaskRequest(params map[string]any) (taskRequest, error) {
	subAgentName, err := requiredString(params, "sub_agent_name")
	if err != nil {
		return taskRequest{}, err
	}

	task, err := optionalString(params, "task")
	if err != nil {
		return taskRequest{}, err
	}
	task = strings.TrimSpace(task)
	if task == "" {
		return taskRequest{}, fmt.Errorf("task must be a non-empty string")
	}

	return taskRequest{SubAgentName: strings.TrimSpace(subAgentName), Task: task}, nil
}

func runTaskRequests(ctx context.Context, runtime agent.ToolRuntime, requests []taskRequest) []TaskTaskResult {
	catalog, err := agents.LoadDefaultCatalog()
	if err != nil {
		results := make([]TaskTaskResult, 0, len(requests))
		for _, req := range requests {
			results = append(results, TaskTaskResult{
				SubAgentName: req.SubAgentName,
				Task:         req.Task,
				Status:       TaskTaskStatusError,
				Error:        fmt.Sprintf("load agent catalog: %v", err),
			})
		}
		return results
	}

	results := make([]TaskTaskResult, len(requests))
	var wg sync.WaitGroup
	wg.Add(len(requests))

	for i := range requests {
		i := i
		go func() {
			defer wg.Done()
			results[i] = runSingleTaskRequest(ctx, runtime, catalog, requests[i], i, runtime.ToolCallID)
		}()
	}

	wg.Wait()
	return results
}

func runSingleTaskRequest(ctx context.Context, runtime agent.ToolRuntime, catalog agents.Catalog, req taskRequest, idx int, parentToolCallID string) TaskTaskResult {
	ret := TaskTaskResult{
		SubAgentName: req.SubAgentName,
		Task:         req.Task,
	}

	agentConfig, ok := catalog.Get(req.SubAgentName)
	if !ok {
		ret.Status = TaskTaskStatusError
		ret.Error = fmt.Sprintf("sub-agent %q not found", req.SubAgentName)
		return ret
	}
	if agentConfig.Type != agents.AgentTypeSubAgent {
		ret.Status = TaskTaskStatusError
		ret.Error = fmt.Sprintf("agent %q is not a sub-agent", req.SubAgentName)
		return ret
	}

	workingDir, _ := os.Getwd()
	systemPrompt := buildSubAgentSystemPrompt(agentConfig.SystemPrompt, workingDir)
	subCtx := agent.Context{
		Model:        runtime.Model,
		Provider:     runtime.Provider,
		SystemPrompt: systemPrompt,
		History:      nil,
		Prompts:      []agent.Message{{Role: agent.RoleUser, Content: req.Task}},
		Tools:        resolveSubAgentTools(agentConfig),
		Approver:     runtime.Approver,
		RunID:        fmt.Sprintf("%s_task_%d", runtime.RunID, idx+1),
		Interactions: agent.NewInteractionManager(),
		Steering:     agent.NewSteeringQueue(),
	}

	var messages []agent.Message
	var runErr error
	emitTaskProgressEvent(ctx, parentToolCallID, idx, req.SubAgentName, req.Task, agent.Event{Type: agent.EventTypeMessage, Data: agent.EventDataMessage{Message: agent.Message{Role: agent.RoleUser, Content: req.Task}}}, agent.Message{})
	agent.RunAgentLoop(ctx, subCtx, func(event agent.Event) {
		switch event.Type {
		case agent.EventTypeAgentEnd:
			if data, ok := event.Data.(agent.EventDataAgentEnd); ok {
				messages = data.Messages
			}
		case agent.EventTypeMessage:
			if data, ok := event.Data.(agent.EventDataMessage); ok {
				emitTaskProgressEvent(ctx, parentToolCallID, idx, req.SubAgentName, req.Task, event, data.Message)
			}
		case agent.EventTypeToolCallStart, agent.EventTypeToolCallEnd, agent.EventTypeMessageDelta, agent.EventTypeThinkingDelta:
			emitTaskProgressEvent(ctx, parentToolCallID, idx, req.SubAgentName, req.Task, event, agent.Message{})
		case agent.EventTypeError:
			switch data := event.Data.(type) {
			case error:
				runErr = data
			case agent.EventDataError:
				runErr = data.Err
			}
		}
	})
	if len(messages) > 0 {
		ret.Messages = append([]agent.Message(nil), messages...)
	}
	if runErr != nil {
		ret.Status = TaskTaskStatusError
		ret.Error = fmt.Sprintf("run sub-agent %q: %v", req.SubAgentName, runErr)
		return ret
	}

	ret.Status = TaskTaskStatusSuccess
	ret.Output = extractFinalAssistantOutput(messages)
	return ret
}

func emitTaskProgressEvent(ctx context.Context, parentToolCallID string, taskIndex int, subAgentName, task string, subEvent agent.Event, fallbackMessage agent.Message) {
	if strings.TrimSpace(parentToolCallID) == "" {
		return
	}
	if subEvent.Type == "" && fallbackMessage.Role == "" {
		return
	}
	if subEvent.Type == "" {
		subEvent = agent.Event{Type: agent.EventTypeMessage, Data: agent.EventDataMessage{Message: fallbackMessage}}
	}
	emit := agent.Event{
		Type:       agent.EventTypeTaskProgress,
		ToolCallID: parentToolCallID,
		Data: agent.EventDataTaskProgress{
			ParentToolCallID: parentToolCallID,
			TaskIndex:        taskIndex,
			SubAgentName:     strings.TrimSpace(subAgentName),
			Task:             strings.TrimSpace(task),
			SubEvent:         subEvent,
		},
	}
	agent.EmitRuntimeEvent(ctx, emit)
}

func extractFinalAssistantOutput(messages []agent.Message) string {
	for i := len(messages) - 1; i >= 0; i-- {
		msg := messages[i]
		if msg.Role != agent.RoleAssistant {
			continue
		}
		content := strings.TrimSpace(msg.Content)
		if content == "" {
			continue
		}
		return content
	}
	return ""
}

func resolveSubAgentTools(agentConfig agents.Agent) map[string]agent.Tool {
	toolsMap := make(map[string]agent.Tool)
	for _, name := range agentConfig.AllowedTools {
		switch strings.TrimSpace(name) {
		case "read":
			toolsMap[name] = NewReadTool()
		case "edit":
			toolsMap[name] = NewEditTool()
		case "edit_plan":
			toolsMap[name] = NewEditPlanTool()
		case "write":
			toolsMap[name] = NewWriteTool()
		case "grep":
			toolsMap[name] = NewGrepTool()
		case "list":
			toolsMap[name] = NewListTool()
		case "glob":
			toolsMap[name] = NewGlobTool()
		case "bash":
			toolsMap[name] = NewBashTool()
		case "skill":
			toolsMap[name] = NewSkillTool()
		case "question":
			toolsMap[name] = NewQuestionTool()
		case "todo_write":
			toolsMap[name] = NewTodoWriteTool()
		case "web_fetch":
			toolsMap[name] = NewWebFetchTool()
		case "web_search":
			toolsMap[name] = NewWebSearchTool()
		}
	}
	return toolsMap
}

func buildSubAgentSystemPrompt(base string, workingDir string) string {
	var parts []string

	base = strings.TrimSpace(base)
	if base != "" {
		parts = append(parts, base)
	}

	skillCatalog, err := loadSkillCatalog()
	if err == nil {
		skillBlock := strings.TrimSpace(skillCatalog.PromptFrontmatterBlock())
		if skillBlock != "" {
			parts = append(parts, skillBlock)
		}
	}

	if globalAgentsMD := readAgentsMD(globalAgentsPath()); globalAgentsMD != "" {
		parts = append(parts, "<global-agents-md>\n"+globalAgentsMD+"\n</global-agents-md>")
	}

	if projectAgentsMD := readAgentsMD(projectAgentsMDPath(workingDir)); projectAgentsMD != "" {
		parts = append(parts, "<project-agents-md>\n"+projectAgentsMD+"\n</project-agents-md>")
	}

	return strings.Join(parts, "\n\n")
}

func globalAgentsPath() string {
	home, err := os.UserHomeDir()
	if err != nil {
		return ""
	}
	return filepath.Join(home, ".agents", "AGENTS.md")
}

func projectAgentsMDPath(workingDir string) string {
	if strings.TrimSpace(workingDir) == "" {
		return ""
	}
	return filepath.Join(workingDir, "AGENTS.md")
}

func readAgentsMD(path string) string {
	path = strings.TrimSpace(path)
	if path == "" {
		return ""
	}
	content, err := os.ReadFile(path)
	if err != nil {
		return ""
	}
	return strings.TrimSpace(string(content))
}

func formatTaskResultData(result TaskResult) string {
	if len(result.Tasks) == 0 {
		return ""
	}
	if len(result.Tasks) == 1 {
		task := result.Tasks[0]
		if task.Status == TaskTaskStatusError {
			return task.Error
		}
		return task.Output
	}

	parts := make([]string, 0, len(result.Tasks))
	for _, task := range result.Tasks {
		if task.Status == TaskTaskStatusError {
			parts = append(parts, fmt.Sprintf("Task %s:\n%s", task.SubAgentName, task.Error))
			continue
		}
		parts = append(parts, fmt.Sprintf("Task %s:\n%s", task.SubAgentName, task.Output))
	}
	return strings.Join(parts, "\n\n")
}
