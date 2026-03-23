# Spec: Question Tool

## Purpose

Question tool provides the LLM to ask users questions. For example, if LLM needs more context, or asks for preference.

## Parameters

Question tool takes several parameters:

1. A question. It should has a "title", an optional "content" and "content type".
   1. "content type" is reserved for future rendering.
2. An array of options. Each option should have a "title" and a "description".
3. A boolean "allow_custom_option", specifying whether to support custom input.

## Workflow

- When question tool is called, it should emit a tool call start event.
- TUI should render the question and options in a dialog.
- User should pick an option or provide a custom option if allowed.
- TUI should sends an event to the agent runner / loop to communicate user's answer.

## How to render the tool line

`Question: "<question title>"`

## How to render the question dialog

In the dialog, it should be something like

-----------------------------------------------
-- One empty line
Question Title (bold)

1. Option 1 title
   Option 1 description
2. Option 2 title
   Option 2 description
...
N. Type your own answer (if allow_custom_option is true)
   -- Once picked this option, user can type here.

-- One empty line
-----------------------------------------------

Pressing enter picks an option. If it's not custom option, just submit the answer. For the custom option, pressing enter again submits the anwer.
