use serde_json::Value;

#[derive(Debug, Clone)]
pub struct ToolCallStartView {
    pub line: String,
}

#[derive(Debug, Clone, Copy)]
pub struct ToolPresentation {
    pub tool_name: &'static str,
    pub render_start: fn(&Value) -> ToolCallStartView,
}

pub fn render_tool_start(name: &str, args: &Value) -> ToolCallStartView {
    match presentation_for(name) {
        Some(presentation) => presentation.render_start_view(args),
        None => render_default_start(name, args),
    }
}

fn presentation_for(name: &str) -> Option<ToolPresentation> {
    TOOL_PRESENTATIONS
        .iter()
        .copied()
        .find(|presentation| presentation.tool_name == name)
}

impl ToolPresentation {
    fn render_start_view(&self, args: &Value) -> ToolCallStartView {
        (self.render_start)(args)
    }
}

const TOOL_PRESENTATIONS: &[ToolPresentation] = &[
    ToolPresentation {
        tool_name: "read",
        render_start: render_read_start,
    },
    ToolPresentation {
        tool_name: "write",
        render_start: render_write_start,
    },
    ToolPresentation {
        tool_name: "glob",
        render_start: render_glob_start,
    },
    ToolPresentation {
        tool_name: "grep",
        render_start: render_grep_start,
    },
    ToolPresentation {
        tool_name: "list",
        render_start: render_list_start,
    },
    ToolPresentation {
        tool_name: "bash",
        render_start: render_bash_start,
    },
    ToolPresentation {
        tool_name: "web_fetch",
        render_start: render_web_fetch_start,
    },
    ToolPresentation {
        tool_name: "web_search",
        render_start: render_web_search_start,
    },
    ToolPresentation {
        tool_name: "todo_read",
        render_start: render_todo_read_start,
    },
    ToolPresentation {
        tool_name: "todo_write",
        render_start: render_todo_write_start,
    },
    ToolPresentation {
        tool_name: "question",
        render_start: render_question_start,
    },
    ToolPresentation {
        tool_name: "edit",
        render_start: render_edit_start,
    },
];

fn render_read_start(args: &Value) -> ToolCallStartView {
    render_action_with_field("Read", "path", args)
}

fn render_write_start(args: &Value) -> ToolCallStartView {
    render_action_with_field("Write", "path", args)
}

fn render_glob_start(args: &Value) -> ToolCallStartView {
    let pattern = json_str(args, "pattern").unwrap_or_else(|| "*".to_string());
    let path = json_str(args, "path").unwrap_or_else(|| ".".to_string());
    ToolCallStartView {
        line: format!("Glob \"{}\" in {}", pattern, path),
    }
}

fn render_grep_start(args: &Value) -> ToolCallStartView {
    let pattern = json_str(args, "pattern").unwrap_or_default();
    let path = json_str(args, "path").unwrap_or_else(|| ".".to_string());
    ToolCallStartView {
        line: format!("Grep \"{}\" in {}", pattern, path),
    }
}

fn render_list_start(args: &Value) -> ToolCallStartView {
    ToolCallStartView {
        line: format!(
            "List {}",
            json_str(args, "path").unwrap_or_else(|| ".".to_string())
        ),
    }
}

fn render_bash_start(args: &Value) -> ToolCallStartView {
    let command = json_str(args, "command").unwrap_or_else(|| compact_json(args));
    ToolCallStartView {
        line: format!("Run `{}`", command),
    }
}

fn render_web_fetch_start(args: &Value) -> ToolCallStartView {
    render_action_with_field("Fetch", "url", args)
}

fn render_web_search_start(args: &Value) -> ToolCallStartView {
    render_action_with_field("Search", "query", args)
}

fn render_todo_write_start(args: &Value) -> ToolCallStartView {
    let count = args
        .as_object()
        .and_then(|map| map.get("todos"))
        .and_then(|value| value.as_array())
        .map(|items| items.len())
        .unwrap_or(0);
    ToolCallStartView {
        line: format!("Update TODO list ({count} items)"),
    }
}

fn render_todo_read_start(_args: &Value) -> ToolCallStartView {
    ToolCallStartView {
        line: "Read TODO list".to_string(),
    }
}

fn render_edit_start(args: &Value) -> ToolCallStartView {
    render_action_with_field("Edit", "path", args)
}

fn render_question_start(args: &Value) -> ToolCallStartView {
    let count = args
        .as_object()
        .and_then(|map| map.get("questions"))
        .and_then(|value| value.as_array())
        .map(|items| items.len())
        .unwrap_or(0);
    ToolCallStartView {
        line: format!("Ask {count} question{}", if count == 1 { "" } else { "s" }),
    }
}

fn render_action_with_field(action: &str, key: &str, args: &Value) -> ToolCallStartView {
    ToolCallStartView {
        line: format!(
            "{action} {}",
            json_str(args, key).unwrap_or_else(|| compact_json(args))
        ),
    }
}

fn render_default_start(name: &str, args: &Value) -> ToolCallStartView {
    let arg_preview = compact_json(args);
    if arg_preview == "{}" {
        ToolCallStartView {
            line: title_case(name),
        }
    } else {
        ToolCallStartView {
            line: format!("{} {}", title_case(name), arg_preview),
        }
    }
}

fn json_str(args: &Value, key: &str) -> Option<String> {
    args.as_object()
        .and_then(|map| map.get(key))
        .and_then(|value| value.as_str())
        .map(ToString::to_string)
}

fn compact_json(value: &Value) -> String {
    serde_json::to_string(value).unwrap_or_else(|_| "{}".to_string())
}

fn title_case(name: &str) -> String {
    let mut result = String::new();
    let mut capitalize = true;
    for ch in name.chars() {
        if matches!(ch, '_' | '-' | ' ') {
            if !result.ends_with(' ') {
                result.push(' ');
            }
            capitalize = true;
            continue;
        }
        if capitalize {
            result.extend(ch.to_uppercase());
            capitalize = false;
        } else {
            result.extend(ch.to_lowercase());
        }
    }
    result.trim().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn read_tool_has_path_forwarding() {
        let rendered = render_tool_start("read", &json!({"path":"src/main.rs"}));
        assert_eq!(rendered.line, "Read src/main.rs");
    }

    #[test]
    fn unknown_tool_uses_fallback() {
        let rendered = render_tool_start("custom_tool", &json!({"foo":"bar"}));
        assert_eq!(rendered.line, "Custom Tool {\"foo\":\"bar\"}");
    }
}
