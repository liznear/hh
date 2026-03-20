package agent

type Context struct {
	Model        string
	SystemPrompt string
	History      []Message
	Prompts      []Message
	Tools        map[string]Tool
}
