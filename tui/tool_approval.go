package tui

import (
	"context"
	"encoding/json"
	"fmt"
	"os"
	"path/filepath"
	"sort"
	"strings"
	"sync"
	"time"

	"github.com/liznear/hh/agent"
	"github.com/liznear/hh/config"
)

type toolApprover struct {
	cwd          string
	policyByTool map[string]string
	localConfig  string
	allowedBash  map[string]struct{}
	allowRules   map[string]struct{}
	allowedDirs  map[string]struct{}
	mu           sync.Mutex
}

func newToolApprover(cfg config.Config, cwd string) (*toolApprover, error) {
	absCWD, err := filepath.Abs(strings.TrimSpace(cwd))
	if err != nil {
		return nil, fmt.Errorf("resolve working directory: %w", err)
	}

	approver := &toolApprover{
		cwd:          filepath.Clean(absCWD),
		policyByTool: map[string]string{},
		localConfig:  filepath.Join(absCWD, ".hh", "config.json"),
		allowedBash:  map[string]struct{}{},
		allowRules:   map[string]struct{}{},
		allowedDirs:  map[string]struct{}{},
	}
	for toolName := range cfg.Permission {
		approver.policyByTool[strings.ToLower(strings.TrimSpace(toolName))] = cfg.ToolPermissionPolicy(toolName)
	}

	allowRules, err := approver.loadPersistentAllowRules()
	if err != nil {
		return nil, err
	}
	for _, rule := range allowRules {
		approver.allowRules[rule] = struct{}{}
		if strings.HasPrefix(rule, "Bash(") && strings.HasSuffix(rule, ")") {
			approver.allowedBash[rule] = struct{}{}
		}
	}

	return approver, nil
}

func (a *toolApprover) Approve(ctx context.Context, toolName string, params map[string]any) error {
	if a == nil {
		return nil
	}
	toolName = strings.ToLower(strings.TrimSpace(toolName))

	policy := a.policyForTool(toolName)

	switch policy {
	case "allow":
		return nil
	case "deny":
		return fmt.Errorf("%s is denied by policy", toolName)
	case "ask":
		switch toolName {
		case "write", "edit":
			return a.approveFileWrite(ctx, toolName, params)
		case "bash":
			return a.approveBash(ctx, params)
		default:
			return a.approveGenericTool(ctx, toolName)
		}
	default:
		return nil
	}
}

func (a *toolApprover) approveGenericTool(ctx context.Context, toolName string) error {
	resp, err := agent.RequestInteraction(ctx, agent.InteractionRequest{
		InteractionID: fmt.Sprintf("approval_%s_%d", toolName, time.Now().UnixNano()),
		Kind:          agent.InteractionKindApproval,
		Title:         fmt.Sprintf("Approve %s?", toolName),
		Content:       fmt.Sprintf("%s requested approval", toolName),
		Options: []agent.InteractionOption{
			{ID: "allow_once", Title: "Allow Once", Description: "Allow only this call"},
			{ID: "deny", Title: "Deny", Description: "Deny this call"},
		},
	})
	if err != nil {
		return err
	}
	if resp.SelectedOptionID == "allow_once" {
		return nil
	}
	return fmt.Errorf("user denied %s", toolName)
}

func (a *toolApprover) policyForTool(toolName string) string {
	toolName = strings.ToLower(strings.TrimSpace(toolName))
	if toolName == "" {
		return "allow"
	}
	if policy, ok := a.policyByTool[toolName]; ok {
		return policy
	}
	return "allow"
}

func (a *toolApprover) approveFileWrite(ctx context.Context, toolName string, params map[string]any) error {
	pathValue, ok := params["path"].(string)
	if !ok || strings.TrimSpace(pathValue) == "" {
		return nil
	}

	targetPath, err := a.resolvePath(pathValue)
	if err != nil {
		return fmt.Errorf("resolve path: %w", err)
	}
	if isWithinDir(targetPath, a.cwd) {
		return nil
	}

	targetDir := filepath.Dir(targetPath)
	if a.isAllowedDir(targetDir) {
		return nil
	}

	resp, err := agent.RequestInteraction(ctx, agent.InteractionRequest{
		InteractionID: fmt.Sprintf("approval_%s_%d", toolName, time.Now().UnixNano()),
		Kind:          agent.InteractionKindApproval,
		Title:         fmt.Sprintf("Approve %s outside workspace?", toolName),
		Content:       fmt.Sprintf("%s wants to modify `%s`", toolName, targetPath),
		Options: []agent.InteractionOption{
			{ID: "allow_session", Title: "Allow in Session", Description: fmt.Sprintf("Allow %s for this folder in this session", toolName)},
			{ID: "allow_once", Title: "Allow Once", Description: "Allow only this call"},
			{ID: "deny", Title: "Deny", Description: "Deny this call"},
		},
	})
	if err != nil {
		return err
	}

	switch resp.SelectedOptionID {
	case "allow_session":
		a.addAllowedDir(targetDir)
		return nil
	case "allow_once":
		return nil
	default:
		return fmt.Errorf("user denied %s", toolName)
	}
}

func (a *toolApprover) approveBash(ctx context.Context, params map[string]any) error {
	commandValue, ok := params["command"].(string)
	if !ok {
		return nil
	}
	command := normalizeCommand(commandValue)
	if command == "" {
		return nil
	}

	if a.isAllowedBash(command) {
		return nil
	}

	resp, err := agent.RequestInteraction(ctx, agent.InteractionRequest{
		InteractionID: fmt.Sprintf("approval_bash_%d", time.Now().UnixNano()),
		Kind:          agent.InteractionKindApproval,
		Title:         "Approve bash command?",
		Content:       fmt.Sprintf("bash wants to run `%s`", command),
		Options: []agent.InteractionOption{
			{ID: "allow_always", Title: "Always Allow", Description: "Always allow this command class"},
			{ID: "allow_once", Title: "Allow Once", Description: "Allow only this call"},
			{ID: "deny", Title: "Deny", Description: "Deny this call"},
		},
	})
	if err != nil {
		return err
	}

	switch resp.SelectedOptionID {
	case "allow_always":
		rule, err := bashRuleFromCommand(command)
		if err != nil {
			return err
		}
		if err := a.addPersistentAllowRule(rule); err != nil {
			return err
		}
		return nil
	case "allow_once":
		return nil
	default:
		return fmt.Errorf("user denied bash")
	}
}

func (a *toolApprover) resolvePath(pathValue string) (string, error) {
	if filepath.IsAbs(pathValue) {
		return filepath.Clean(pathValue), nil
	}
	return filepath.Abs(filepath.Join(a.cwd, pathValue))
}

func (a *toolApprover) isAllowedDir(targetDir string) bool {
	a.mu.Lock()
	defer a.mu.Unlock()
	targetDir = filepath.Clean(targetDir)
	for dir := range a.allowedDirs {
		if isWithinDir(targetDir, dir) {
			return true
		}
	}
	return false
}

func (a *toolApprover) addAllowedDir(targetDir string) {
	a.mu.Lock()
	defer a.mu.Unlock()
	a.allowedDirs[filepath.Clean(targetDir)] = struct{}{}
}

func (a *toolApprover) isAllowedBash(command string) bool {
	a.mu.Lock()
	defer a.mu.Unlock()
	for rule := range a.allowedBash {
		if bashRuleMatches(rule, command) {
			return true
		}
	}
	return false
}

func (a *toolApprover) addPersistentAllowRule(rule string) error {
	a.mu.Lock()
	defer a.mu.Unlock()
	a.allowedBash[rule] = struct{}{}
	a.allowRules[rule] = struct{}{}

	rules := make([]string, 0, len(a.allowRules))
	for r := range a.allowRules {
		rules = append(rules, r)
	}
	sort.Strings(rules)

	content, err := readJSONFile(a.localConfig)
	if err != nil {
		return err
	}
	permissions, _ := content["permissions"].(map[string]any)
	if permissions == nil {
		permissions = map[string]any{}
	}
	allow := make([]any, 0, len(rules))
	for _, r := range rules {
		allow = append(allow, r)
	}
	permissions["allow"] = allow
	content["permissions"] = permissions

	buf, err := json.MarshalIndent(content, "", "  ")
	if err != nil {
		return fmt.Errorf("serialize %s: %w", a.localConfig, err)
	}
	buf = append(buf, '\n')

	if err := os.MkdirAll(filepath.Dir(a.localConfig), 0o755); err != nil {
		return fmt.Errorf("create .hh directory: %w", err)
	}
	if err := os.WriteFile(a.localConfig, buf, 0o644); err != nil {
		return fmt.Errorf("write %s: %w", a.localConfig, err)
	}
	return nil
}

func (a *toolApprover) loadPersistentAllowRules() ([]string, error) {
	content, err := readJSONFile(a.localConfig)
	if err != nil {
		return nil, err
	}
	permissions, _ := content["permissions"].(map[string]any)
	if permissions == nil {
		return nil, nil
	}
	rawAllow, _ := permissions["allow"].([]any)
	if rawAllow == nil {
		return nil, nil
	}

	rules := make([]string, 0, len(rawAllow))
	for _, item := range rawAllow {
		rule, ok := item.(string)
		if !ok {
			continue
		}
		rule = strings.TrimSpace(rule)
		if rule != "" {
			rules = append(rules, rule)
		}
	}
	return rules, nil
}

func readJSONFile(path string) (map[string]any, error) {
	buf, err := os.ReadFile(path)
	if err != nil {
		if os.IsNotExist(err) {
			return map[string]any{}, nil
		}
		return nil, fmt.Errorf("read %s: %w", path, err)
	}
	if strings.TrimSpace(string(buf)) == "" {
		return map[string]any{}, nil
	}
	var out map[string]any
	if err := json.Unmarshal(buf, &out); err != nil {
		return nil, fmt.Errorf("parse %s: %w", path, err)
	}
	if out == nil {
		return map[string]any{}, nil
	}
	return out, nil
}

func isWithinDir(target, parent string) bool {
	target = filepath.Clean(target)
	parent = filepath.Clean(parent)
	rel, err := filepath.Rel(parent, target)
	if err != nil {
		return false
	}
	if rel == "." {
		return true
	}
	return rel != ".." && !strings.HasPrefix(rel, ".."+string(filepath.Separator))
}

func normalizeCommand(command string) string {
	parts := strings.Fields(strings.TrimSpace(command))
	return strings.Join(parts, " ")
}

func bashRuleFromCommand(command string) (string, error) {
	parts := strings.Fields(command)
	if len(parts) == 0 {
		return "", fmt.Errorf("invalid bash command")
	}
	return fmt.Sprintf("Bash(%s:*)", parts[0]), nil
}

func bashRuleMatches(rule, command string) bool {
	if !strings.HasPrefix(rule, "Bash(") || !strings.HasSuffix(rule, ")") {
		return false
	}
	inner := strings.TrimSuffix(strings.TrimPrefix(rule, "Bash("), ")")
	if inner == "" {
		return false
	}
	command = normalizeCommand(command)
	if command == "" {
		return false
	}

	if strings.HasSuffix(inner, ":*") {
		prefix := strings.TrimSpace(strings.TrimSuffix(inner, ":*"))
		if prefix == "" {
			return false
		}
		return command == prefix || strings.HasPrefix(command, prefix+" ")
	}

	return command == strings.TrimSpace(inner)
}
