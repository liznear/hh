package session

import (
	"bufio"
	"encoding/json"
	"fmt"
	"os"
	"path/filepath"
	"sort"
	"strings"
	"time"

	"github.com/liznear/hh/tools"
)

type Storage struct {
	baseDir string
}

func NewStorage(baseDir string) (*Storage, error) {
	if err := os.MkdirAll(baseDir, 0755); err != nil {
		return nil, fmt.Errorf("failed to create session directory: %w", err)
	}
	return &Storage{baseDir: baseDir}, nil
}

func DefaultStorageDir() (string, error) {
	configDir, err := os.UserConfigDir()
	if err != nil {
		return "", err
	}

	cwd, err := os.Getwd()
	if err != nil {
		return "", fmt.Errorf("failed to get cwd: %w", err)
	}

	projectName := strings.ReplaceAll(cwd, "/", "--")
	if projectName == "" {
		return "", fmt.Errorf("empty project name from cwd")
	}

	return filepath.Join(configDir, "hh", "sessions", projectName), nil
}

func (s *Storage) SaveMeta(state *State) error {
	if state.ID == "" {
		return fmt.Errorf("session ID is required")
	}

	meta := Meta{
		ID:           state.ID,
		Title:        state.Title,
		CreatedAt:    state.CreatedAt,
		CurrentModel: state.CurrentModel,
		TodoItems:    append([]TodoItem(nil), state.TodoItems...),
		TurnCount:    len(state.Turns),
	}

	path := s.metaPath(state.ID)
	data, err := json.MarshalIndent(meta, "", "  ")
	if err != nil {
		return fmt.Errorf("failed to marshal session meta: %w", err)
	}

	if err := os.WriteFile(path, data, 0644); err != nil {
		return fmt.Errorf("failed to write session meta file: %w", err)
	}

	return nil
}

func (s *Storage) Save(state *State) error {
	if state == nil {
		return fmt.Errorf("state is required")
	}
	if err := s.SaveMeta(state); err != nil {
		return err
	}

	path := s.itemsPath(state.ID)
	f, err := os.OpenFile(path, os.O_CREATE|os.O_WRONLY|os.O_TRUNC, 0644)
	if err != nil {
		return fmt.Errorf("failed to open items file: %w", err)
	}
	defer f.Close()

	for i, turn := range state.Turns {
		if turn == nil {
			continue
		}
		turnNum := i + 1
		for _, item := range turn.Items {
			if err := s.appendItemToWriter(f, turnNum, item); err != nil {
				return err
			}
		}
	}

	return nil
}

func (s *Storage) AppendItem(sessionID string, turnNum int, item Item) error {
	path := s.itemsPath(sessionID)
	f, err := os.OpenFile(path, os.O_APPEND|os.O_CREATE|os.O_WRONLY, 0644)
	if err != nil {
		return fmt.Errorf("failed to open items file: %w", err)
	}
	defer f.Close()

	return s.appendItemToWriter(f, turnNum, item)
}

func (s *Storage) appendItemToWriter(f *os.File, turnNum int, item Item) error {
	if item == nil {
		return nil
	}

	raw := marshalItemToRaw(item)
	if raw == nil {
		return fmt.Errorf("unsupported item type")
	}

	timestamp := item.Timestamp()
	if timestamp.IsZero() {
		timestamp = time.Now()
		item.setTimestamp(timestamp)
	}

	line, err := json.Marshal(struct {
		TurnNum   int             `json:"turn_num"`
		Timestamp time.Time       `json:"timestamp"`
		Item      json.RawMessage `json:"item"`
	}{
		TurnNum:   turnNum,
		Timestamp: timestamp,
		Item:      raw,
	})
	if err != nil {
		return fmt.Errorf("failed to encode item entry: %w", err)
	}

	if _, err := f.WriteString(string(line) + "\n"); err != nil {
		return fmt.Errorf("failed to write item entry: %w", err)
	}

	return nil
}

func (s *Storage) Load(id string) (*State, error) {
	meta, err := s.LoadMeta(id)
	if err != nil {
		return nil, err
	}

	entries, err := s.LoadItems(id)
	if err != nil {
		return nil, err
	}

	state := &State{ID: meta.ID, Title: meta.Title, CreatedAt: meta.CreatedAt, CurrentModel: meta.CurrentModel, TodoItems: append([]TodoItem(nil), meta.TodoItems...)}
	if strings.TrimSpace(state.Title) == "" {
		state.Title = "Untitled Session"
	}

	turnsByNum := map[int]*Turn{}
	maxTurnNum := 0
	for _, entry := range entries {
		if entry.TurnNum <= 0 {
			continue
		}
		if entry.TurnNum > maxTurnNum {
			maxTurnNum = entry.TurnNum
		}

		turn := turnsByNum[entry.TurnNum]
		if turn == nil {
			turn = &Turn{ID: fmt.Sprintf("turn-%d", entry.TurnNum), ModelName: state.CurrentModel}
			turnsByNum[entry.TurnNum] = turn
		}

		if !entry.Timestamp.IsZero() {
			entry.Item.setTimestamp(entry.Timestamp)
		}
		turn.AddItem(entry.Item)

		if start, ok := entry.Item.(*Start); ok {
			if start.Model != "" {
				turn.ModelName = start.Model
			}
			if turn.StartedAt.IsZero() {
				turn.StartedAt = entry.Timestamp
			}
		}
		if _, ok := entry.Item.(*End); ok {
			ts := entry.Timestamp
			if !ts.IsZero() {
				turn.EndedAt = &ts
			}
		}
		if tc, ok := entry.Item.(*ToolCallItem); ok && tc.Result != nil {
			tc.Result.Result = decodeToolResult(tc.Name, tc.Result.Result)
		}
	}

	turnCount := max(meta.TurnCount, maxTurnNum)
	if turnCount > 0 {
		state.Turns = make([]*Turn, turnCount)
		for i := 1; i <= turnCount; i++ {
			turn, ok := turnsByNum[i]
			if !ok {
				turn = &Turn{ID: fmt.Sprintf("turn-%d", i), ModelName: state.CurrentModel}
			}
			if turn.ModelName == "" {
				turn.ModelName = state.CurrentModel
			}
			state.Turns[i-1] = turn
		}
	}

	return state, nil
}

func (s *Storage) LoadMeta(id string) (*Meta, error) {
	path := s.metaPath(id)
	data, err := os.ReadFile(path)
	if err != nil {
		return nil, fmt.Errorf("failed to read session meta file: %w", err)
	}

	var meta Meta
	if err := json.Unmarshal(data, &meta); err != nil {
		return nil, fmt.Errorf("failed to unmarshal session meta: %w", err)
	}

	return &meta, nil
}

func (s *Storage) LoadItems(id string) ([]ItemEntry, error) {
	path := s.itemsPath(id)
	f, err := os.Open(path)
	if err != nil {
		if os.IsNotExist(err) {
			return nil, nil
		}
		return nil, fmt.Errorf("failed to open items file: %w", err)
	}
	defer f.Close()

	var entries []ItemEntry
	scanner := bufio.NewScanner(f)
	for scanner.Scan() {
		line := scanner.Bytes()
		if len(line) == 0 {
			continue
		}

		var rawEntry struct {
			TurnNum   int             `json:"turn_num"`
			Timestamp time.Time       `json:"timestamp"`
			Item      json.RawMessage `json:"item"`
		}
		if err := json.Unmarshal(line, &rawEntry); err != nil {
			continue
		}

		item, err := unmarshalItemFromRaw(rawEntry.Item)
		if err != nil {
			continue
		}

		entries = append(entries, ItemEntry{
			TurnNum:   rawEntry.TurnNum,
			Timestamp: rawEntry.Timestamp,
			Item:      item,
		})
	}

	if err := scanner.Err(); err != nil {
		return nil, fmt.Errorf("failed to read items file: %w", err)
	}

	return entries, nil
}

func (s *Storage) Delete(id string) error {
	if err := os.Remove(s.metaPath(id)); err != nil && !os.IsNotExist(err) {
		return err
	}
	if err := os.Remove(s.itemsPath(id)); err != nil && !os.IsNotExist(err) {
		return err
	}
	return nil
}

type Meta struct {
	ID           string     `json:"id"`
	Title        string     `json:"title,omitempty"`
	CreatedAt    time.Time  `json:"created_at"`
	CurrentModel string     `json:"current_model"`
	TodoItems    []TodoItem `json:"todo_items,omitempty"`
	TurnCount    int        `json:"turn_count"`
}

type ItemEntry struct {
	TurnNum   int
	Timestamp time.Time
	Item      Item
}

func (s *Storage) List() ([]Meta, error) {
	entries, err := os.ReadDir(s.baseDir)
	if err != nil {
		return nil, fmt.Errorf("failed to read session directory: %w", err)
	}

	var metas []Meta
	for _, entry := range entries {
		if entry.IsDir() {
			continue
		}
		name := entry.Name()
		if !strings.HasSuffix(name, ".meta.json") {
			continue
		}

		id := name[:len(name)-10]
		path := s.metaPath(id)
		data, err := os.ReadFile(path)
		if err != nil {
			continue
		}

		var meta Meta
		if err := json.Unmarshal(data, &meta); err != nil {
			continue
		}

		metas = append(metas, meta)
	}

	sort.Slice(metas, func(i, j int) bool {
		return metas[i].CreatedAt.After(metas[j].CreatedAt)
	})

	return metas, nil
}

func (s *Storage) metaPath(id string) string {
	return filepath.Join(s.baseDir, id+".meta.json")
}

func (s *Storage) itemsPath(id string) string {
	return filepath.Join(s.baseDir, id+".jsonl")
}

func marshalItemToRaw(item Item) json.RawMessage {
	var typeStr string
	var data any

	switch v := item.(type) {
	case *Start:
		typeStr = "start"
		data = v
	case *UserMessage:
		typeStr = "user_message"
		data = v
	case *ShellMessage:
		typeStr = "shell_message"
		data = v
	case *AssistantMessage:
		typeStr = "assistant_message"
		data = v
	case *ThinkingBlock:
		typeStr = "thinking_block"
		data = v
	case *ToolCallItem:
		typeStr = "tool_call"
		data = v
	case *ErrorItem:
		typeStr = "error"
		data = v
	case *End:
		typeStr = "end"
		data = v
	default:
		return nil
	}

	wrapped := struct {
		Type string `json:"type"`
		Data any    `json:"data"`
	}{Type: typeStr, Data: data}
	b, _ := json.Marshal(wrapped)
	return b
}

func unmarshalItemFromRaw(raw json.RawMessage) (Item, error) {
	var typeOnly struct {
		Type string `json:"type"`
	}
	if err := json.Unmarshal(raw, &typeOnly); err != nil {
		return nil, err
	}

	switch typeOnly.Type {
	case "start":
		var start Start
		if err := json.Unmarshal(raw, &struct {
			Data *Start `json:"data"`
		}{Data: &start}); err != nil {
			return nil, err
		}
		return &start, nil
	case "user_message":
		var msg UserMessage
		if err := json.Unmarshal(raw, &struct {
			Data *UserMessage `json:"data"`
		}{Data: &msg}); err != nil {
			return nil, err
		}
		return &msg, nil
	case "shell_message":
		var msg ShellMessage
		if err := json.Unmarshal(raw, &struct {
			Data *ShellMessage `json:"data"`
		}{Data: &msg}); err != nil {
			return nil, err
		}
		return &msg, nil
	case "assistant_message":
		var msg AssistantMessage
		if err := json.Unmarshal(raw, &struct {
			Data *AssistantMessage `json:"data"`
		}{Data: &msg}); err != nil {
			return nil, err
		}
		return &msg, nil
	case "thinking_block":
		var block ThinkingBlock
		if err := json.Unmarshal(raw, &struct {
			Data *ThinkingBlock `json:"data"`
		}{Data: &block}); err != nil {
			return nil, err
		}
		return &block, nil
	case "tool_call":
		var tc ToolCallItem
		if err := json.Unmarshal(raw, &struct {
			Data *ToolCallItem `json:"data"`
		}{Data: &tc}); err != nil {
			return nil, err
		}
		if tc.Result != nil {
			tc.Result.Result = decodeToolResult(tc.Name, tc.Result.Result)
		}
		return &tc, nil
	case "error":
		var errItem ErrorItem
		if err := json.Unmarshal(raw, &struct {
			Data *ErrorItem `json:"data"`
		}{Data: &errItem}); err != nil {
			return nil, err
		}
		return &errItem, nil
	case "end":
		var end End
		if err := json.Unmarshal(raw, &struct {
			Data *End `json:"data"`
		}{Data: &end}); err != nil {
			return nil, err
		}
		return &end, nil
	default:
		return nil, fmt.Errorf("unknown item type: %s", typeOnly.Type)
	}
}

func decodeToolResult(name string, raw any) any {
	if raw == nil {
		return nil
	}
	if _, ok := raw.(interface{ Summary() string }); ok {
		return raw
	}

	mapped, ok := raw.(map[string]any)
	if !ok {
		return raw
	}

	switch strings.ToLower(name) {
	case "read":
		return decodeMappedResult[tools.ReadResult](mapped, raw)
	case "edit":
		return decodeMappedResult[tools.EditResult](mapped, raw)
	case "write":
		return decodeMappedResult[tools.WriteResult](mapped, raw)
	case "grep":
		return decodeMappedResult[tools.GrepResult](mapped, raw)
	case "list", "ls":
		return decodeMappedResult[tools.ListResult](mapped, raw)
	case "todo_write":
		return decodeMappedResult[tools.TodoWriteResult](mapped, raw)
	case "skill":
		return decodeMappedResult[tools.SkillResult](mapped, raw)
	case "task":
		return decodeMappedResult[tools.TaskResult](mapped, raw)
	case "web_fetch":
		return decodeMappedResult[tools.WebFetchResult](mapped, raw)
	case "web_search":
		return decodeMappedResult[tools.WebSearchResult](mapped, raw)
	default:
		return raw
	}
}

func decodeMappedResult[T any](mapped map[string]any, fallback any) any {
	buf, err := json.Marshal(mapped)
	if err != nil {
		return fallback
	}
	var out T
	if err := json.Unmarshal(buf, &out); err != nil {
		return fallback
	}
	return out
}
