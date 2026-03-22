package tui

import (
	"context"

	tea "charm.land/bubbletea/v2"
	"github.com/liznear/hh/agent"
)

type agentStreamStartedMsg struct {
	ch <-chan tea.Msg
}

type agentEventMsg struct {
	event agent.Event
}

type agentRunDoneMsg struct {
	err error
}

type streamBatchMsg struct {
	events  []agent.Event
	done    bool
	doneErr error
}

func startAgentStreamCmd(runner *agent.AgentRunner, prompt string) tea.Cmd {
	return startAgentStreamCmdWithContext(context.Background(), runner, prompt)
}

func startAgentStreamCmdWithContext(ctx context.Context, runner *agent.AgentRunner, prompt string) tea.Cmd {
	return func() tea.Msg {
		ch := make(chan tea.Msg)
		go func() {
			err := runner.Run(ctx, agent.Input{Content: prompt, Type: "text"}, func(e agent.Event) {
				ch <- agentEventMsg{event: e}
			})
			ch <- agentRunDoneMsg{err: err}
			close(ch)
		}()
		return agentStreamStartedMsg{ch: ch}
	}
}

func waitForStreamCmd(ch <-chan tea.Msg) tea.Cmd {
	return func() tea.Msg {
		if ch == nil {
			return nil
		}
		msg, ok := <-ch
		if !ok {
			return nil
		}

		switch first := msg.(type) {
		case agentEventMsg:
			events := []agent.Event{first.event}
			for i := 1; i < streamBatchMaxEvents; i++ {
				select {
				case next, ok := <-ch:
					if !ok {
						return streamBatchMsg{events: events}
					}
					switch v := next.(type) {
					case agentEventMsg:
						events = append(events, v.event)
					case agentRunDoneMsg:
						return streamBatchMsg{events: events, done: true, doneErr: v.err}
					default:
						return streamBatchMsg{events: events}
					}
				default:
					return streamBatchMsg{events: events}
				}
			}
			return streamBatchMsg{events: events}

		case agentRunDoneMsg:
			return streamBatchMsg{done: true, doneErr: first.err}

		default:
			return msg
		}
	}
}
