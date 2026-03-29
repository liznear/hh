package session

import (
	"strings"
	"time"

	"github.com/liznear/hh/agent"
)

type State struct {
	ID           string
	Title        string
	CreatedAt    time.Time
	CurrentModel string
	TodoItems    []TodoItem
	Turns        []*Turn
}

func NewState(modelName string) *State {
	return &State{
		ID:           generateID(),
		Title:        "Untitled Session",
		CreatedAt:    time.Now(),
		CurrentModel: modelName,
		TodoItems:    nil,
		Turns:        nil,
	}
}

func (s *State) SetTitle(title string) {
	title = trimTitle(title)
	if title == "" {
		title = "Untitled Session"
	}
	s.Title = title
}

func trimTitle(title string) string {
	title = strings.TrimSpace(title)
	if title == "" {
		return ""
	}
	lines := strings.Split(title, "\n")
	title = strings.TrimSpace(lines[0])
	title = strings.Trim(title, `"'`)
	if title == "" {
		return ""
	}
	runes := []rune(title)
	if len(runes) > 80 {
		title = string(runes[:80])
	}
	return strings.TrimSpace(title)
}

type TodoStatus string

const (
	TodoStatusPending   TodoStatus = "pending"
	TodoStatusWIP       TodoStatus = "wip"
	TodoStatusCompleted TodoStatus = "completed"
	TodoStatusCancelled TodoStatus = "cancelled"
)

type TodoItem struct {
	Content string     `json:"content"`
	Status  TodoStatus `json:"status"`
}

func (s *State) SetTodoItems(items []TodoItem) {
	if len(items) == 0 {
		s.TodoItems = nil
		return
	}
	cloned := make([]TodoItem, len(items))
	copy(cloned, items)
	s.TodoItems = cloned
}

func (s *State) SetModel(modelName string) {
	s.CurrentModel = modelName
}

func (s *State) StartTurn() *Turn {
	startedAt := time.Now()
	turn := &Turn{
		ID:        generateTurnID(),
		ModelName: s.CurrentModel,
		StartedAt: startedAt,
	}
	turn.AddItem(&Start{Model: s.CurrentModel})
	s.Turns = append(s.Turns, turn)
	return turn
}

func (s *State) CurrentTurn() *Turn {
	if len(s.Turns) == 0 {
		return nil
	}
	for i := len(s.Turns) - 1; i >= 0; i-- {
		if s.Turns[i] != nil {
			return s.Turns[i]
		}
	}
	return nil
}

func (s *State) AddItem(item Item) {
	turn := s.CurrentTurn()
	if turn == nil {
		turn = s.StartTurn()
	}
	turn.AddItem(item)
}

func (s *State) LastItem() Item {
	turn := s.CurrentTurn()
	if turn == nil {
		return nil
	}
	return turn.LastItem()
}

func (s *State) AllItems() []Item {
	var items []Item
	for _, turn := range s.Turns {
		if turn == nil {
			continue
		}
		items = append(items, turn.Items...)
	}
	return items
}

func (s *State) CurrentTurnItems() []Item {
	turn := s.CurrentTurn()
	if turn == nil {
		return nil
	}
	return turn.Items
}

func (s *State) ItemCount() int {
	count := 0
	for _, turn := range s.Turns {
		if turn == nil {
			continue
		}
		count += len(turn.Items)
	}
	return count
}

type Turn struct {
	ID        string
	ModelName string
	StartedAt time.Time
	EndedAt   *time.Time
	Items     []Item
}

func (t *Turn) AddItem(item Item) {
	if item == nil {
		return
	}
	if item.Timestamp().IsZero() {
		item.setTimestamp(time.Now())
	}
	if start, ok := item.(*Start); ok {
		if start.Model != "" {
			t.ModelName = start.Model
		}
		if t.StartedAt.IsZero() {
			t.StartedAt = item.Timestamp()
		}
	}
	if _, ok := item.(*End); ok {
		ts := item.Timestamp()
		t.EndedAt = &ts
	}
	t.Items = append(t.Items, item)
}

func (t *Turn) LastItem() Item {
	if len(t.Items) == 0 {
		return nil
	}
	return t.Items[len(t.Items)-1]
}

func (t *Turn) End() {
	t.EndWithStatus("")
}

func (t *Turn) EndWithStatus(status string) {
	if t.EndedAt != nil {
		return
	}
	now := time.Now()
	t.EndedAt = &now
	t.AddItem(&End{Status: status})
}

func generateTurnID() string {
	return time.Now().Format("150405.000")
}

type ItemType int

const (
	ItemTypeStart ItemType = iota
	ItemTypeUserMessage
	ItemTypeShellMessage
	ItemTypeAssistantMessage
	ItemTypeThinkingBlock
	ItemTypeToolCall
	ItemTypeError
	ItemTypeEnd
	ItemTypeBTWExchange
	ItemTypeCompactionMarker
)

type Item interface {
	Type() ItemType
	Timestamp() time.Time
	setTimestamp(ts time.Time)
}

type baseItem struct {
	timestamp time.Time
}

func (i *baseItem) Timestamp() time.Time {
	return i.timestamp
}

func (i *baseItem) setTimestamp(ts time.Time) {
	i.timestamp = ts
}

type Start struct {
	baseItem
	Model string `json:"model"`
}

func (s *Start) Type() ItemType { return ItemTypeStart }

type UserMessage struct {
	baseItem
	Content string
	Queued  bool `json:"queued,omitempty"`
}

func (m *UserMessage) Type() ItemType { return ItemTypeUserMessage }

type ShellMessage struct {
	baseItem
	Command string
	Output  string
}

func (m *ShellMessage) Type() ItemType { return ItemTypeShellMessage }

type AssistantMessage struct {
	baseItem
	Content string
}

func (m *AssistantMessage) Type() ItemType { return ItemTypeAssistantMessage }

func (m *AssistantMessage) Append(delta string) {
	m.Content += delta
}

type ThinkingBlock struct {
	baseItem
	Content string
}

func (b *ThinkingBlock) Type() ItemType { return ItemTypeThinkingBlock }

func (b *ThinkingBlock) Append(delta string) {
	b.Content += delta
}

type ToolCallStatus int

const (
	ToolCallStatusPending ToolCallStatus = iota
	ToolCallStatusSuccess
	ToolCallStatusError
)

type ToolCallItem struct {
	baseItem
	ID        string
	Name      string
	Arguments string
	Result    *ToolCallResult
	Status    ToolCallStatus
}

func (t *ToolCallItem) Type() ItemType { return ItemTypeToolCall }

func (t *ToolCallItem) Complete(result agent.ToolResult) {
	t.Result = &ToolCallResult{
		IsErr:       result.IsErr,
		Result:      result.Result,
		ContentType: result.ContentType,
		Data:        result.Data,
	}
	if result.IsErr {
		t.Status = ToolCallStatusError
	} else {
		t.Status = ToolCallStatusSuccess
	}
}

func (t *ToolCallItem) ResultSummary() string {
	if t == nil || t.Result == nil {
		return ""
	}
	if s, ok := t.Result.Result.(interface{ Summary() string }); ok {
		return s.Summary()
	}
	return ""
}

type ToolCallResult struct {
	IsErr       bool
	Result      any
	ContentType string
	Data        string
}

type ErrorItem struct {
	baseItem
	Message string
}

func (e *ErrorItem) Type() ItemType { return ItemTypeError }

type End struct {
	baseItem
	Status string `json:"status,omitempty"`
}

func (e *End) Type() ItemType { return ItemTypeEnd }

type BTWExchange struct {
	baseItem
	Question string
	Answer   string
}

func (b *BTWExchange) Type() ItemType { return ItemTypeBTWExchange }

func (b *BTWExchange) AppendAnswer(delta string) {
	b.Answer += delta
}

type CompactionMarker struct {
	baseItem
}

func (c *CompactionMarker) Type() ItemType { return ItemTypeCompactionMarker }

func generateID() string {
	return time.Now().Format("20060102-150405")
}
