package agent

type Context struct {
	Model        string
	Provider     Provider
	SystemPrompt string
	History      []Message
	Prompts      []Message
	Tools        map[string]Tool
	RunID        string
	Interactions *InteractionManager
	Steering     *SteeringQueue
}
