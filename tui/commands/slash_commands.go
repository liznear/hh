package commands

import "strings"

type Command struct {
	Name    string
	Summary string
	Action  string
}

type Invocation struct {
	Name    string
	Args    []string
	ArgsRaw string
}

const ActionNewSession = "new_session"
const ActionModelPicker = "model_picker"
const ActionResumeSession = "resume_session"

func BuiltIn() map[string]Command {
	commands := []Command{
		{
			Name:    "new",
			Summary: "start a new session",
			Action:  ActionNewSession,
		},
		{
			Name:    "model",
			Summary: "pick the active model",
			Action:  ActionModelPicker,
		},
		{
			Name:    "resume",
			Summary: "list recent sessions for this path",
			Action:  ActionResumeSession,
		},
	}

	index := make(map[string]Command, len(commands))
	for _, cmd := range commands {
		index[cmd.Name] = cmd
	}
	return index
}

func ParseInvocation(prompt string) (Invocation, bool) {
	prompt = strings.TrimSpace(prompt)
	if !strings.HasPrefix(prompt, "/") {
		return Invocation{}, false
	}

	body := strings.TrimSpace(strings.TrimPrefix(prompt, "/"))
	if body == "" {
		return Invocation{}, false
	}

	name, argsRaw, hasArgs := strings.Cut(body, " ")
	name = strings.ToLower(strings.TrimSpace(name))
	if name == "" {
		return Invocation{}, false
	}

	inv := Invocation{Name: name}
	if hasArgs {
		inv.ArgsRaw = strings.TrimSpace(argsRaw)
		if inv.ArgsRaw != "" {
			inv.Args = strings.Fields(inv.ArgsRaw)
		}
	}
	return inv, true
}
