use crate::core::{QuestionAnswers, QuestionPrompt};
use crate::tool::{Tool, ToolResult, ToolSchema};
use async_trait::async_trait;
use serde::Deserialize;
use serde_json::{Value, json};

pub struct QuestionTool;

#[derive(Debug, Deserialize)]
pub struct QuestionArgs {
    pub questions: Vec<QuestionPrompt>,
}

pub fn parse_question_args(args: Value) -> anyhow::Result<QuestionArgs> {
    let parsed: QuestionArgs = serde_json::from_value(args)?;
    if parsed.questions.is_empty() {
        anyhow::bail!("questions must not be empty");
    }

    for (index, question) in parsed.questions.iter().enumerate() {
        if question.question.trim().is_empty() {
            anyhow::bail!("questions[{index}].question must not be empty");
        }
        if question.header.trim().is_empty() {
            anyhow::bail!("questions[{index}].header must not be empty");
        }
        if question.options.is_empty() {
            anyhow::bail!("questions[{index}].options must not be empty");
        }
        for (opt_index, option) in question.options.iter().enumerate() {
            if option.label.trim().is_empty() {
                anyhow::bail!("questions[{index}].options[{opt_index}].label must not be empty");
            }
        }
    }

    Ok(parsed)
}

pub fn question_result(questions: &[QuestionPrompt], answers: QuestionAnswers) -> ToolResult {
    let formatted = questions
        .iter()
        .enumerate()
        .map(|(idx, question)| {
            let answer = answers
                .get(idx)
                .filter(|items| !items.is_empty())
                .map(|items| items.join(", "))
                .unwrap_or_else(|| "Unanswered".to_string());
            format!("\"{}\"=\"{}\"", question.question, answer)
        })
        .collect::<Vec<_>>()
        .join(", ");

    ToolResult::ok_json_typed(
        format!(
            "Asked {} question{}",
            questions.len(),
            if questions.len() == 1 { "" } else { "s" }
        ),
        "application/vnd.hh.question+json",
        json!({
            "answers": answers,
            "message": format!(
                "User has answered your questions: {formatted}. You can now continue with the user's answers in mind."
            ),
        }),
    )
}

#[async_trait]
impl Tool for QuestionTool {
    fn schema(&self) -> ToolSchema {
        ToolSchema {
            name: "question".to_string(),
            description: "Ask the user questions during execution.".to_string(),
            capability: Some("question".to_string()),
            mutating: Some(false),
            parameters: json!({
                "type": "object",
                "properties": {
                    "questions": {
                        "type": "array",
                        "description": "Questions to ask",
                        "items": {
                            "type": "object",
                            "properties": {
                                "question": {
                                    "type": "string",
                                    "description": "Complete question"
                                },
                                "header": {
                                    "type": "string",
                                    "description": "Very short label (max 30 chars)",
                                    "maxLength": 30
                                },
                                "options": {
                                    "type": "array",
                                    "description": "Available choices",
                                    "items": {
                                        "type": "object",
                                        "properties": {
                                            "label": {
                                                "type": "string",
                                                "description": "Display text (1-5 words, concise)",
                                                "maxLength": 30
                                            },
                                            "description": {
                                                "type": "string",
                                                "description": "Explanation of choice"
                                            }
                                        },
                                        "required": ["label", "description"],
                                        "additionalProperties": false
                                    }
                                },
                                "multiple": {
                                    "type": "boolean",
                                    "description": "Allow selecting multiple choices"
                                },
                                "custom": {
                                    "type": "boolean",
                                    "description": "Allow typing a custom answer"
                                }
                            },
                            "required": ["question", "header", "options"],
                            "additionalProperties": false
                        }
                    }
                },
                "required": ["questions"],
                "additionalProperties": false
            }),
        }
    }

    async fn execute(&self, _args: Value) -> ToolResult {
        ToolResult::err_text(
            "question_not_available",
            "question tool can only be executed through the interactive agent loop",
        )
    }
}
