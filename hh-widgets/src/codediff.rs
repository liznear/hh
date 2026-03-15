/// Immutable codediff view model.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
#[non_exhaustive]
pub struct CodeDiffBlock {
    pub unified_diff: String,
}

impl CodeDiffBlock {
    pub fn new(unified_diff: impl Into<String>) -> Self {
        Self {
            unified_diff: unified_diff.into(),
        }
    }
}

/// Codediff rendering options.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct CodeDiffOptions {
    pub max_rendered_lines: usize,
    pub max_rendered_chars: usize,
    pub show_file_headers: bool,
}

impl Default for CodeDiffOptions {
    fn default() -> Self {
        Self {
            max_rendered_lines: 120,
            max_rendered_chars: 8_000,
            show_file_headers: true,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CodeDiffLineKind {
    Add,
    Remove,
    Meta,
    Context,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CodeDiffLine {
    pub kind: CodeDiffLineKind,
    pub text: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct CodeDiffRender {
    pub lines: Vec<CodeDiffLine>,
    pub truncated: bool,
}

pub fn render_unified_diff(block: &CodeDiffBlock, options: &CodeDiffOptions) -> CodeDiffRender {
    let mut out = Vec::new();
    let mut rendered_chars = 0usize;
    let mut truncated = false;

    for (index, raw_line) in block.unified_diff.lines().enumerate() {
        let line_chars = raw_line.chars().count();
        if index >= options.max_rendered_lines
            || rendered_chars.saturating_add(line_chars) > options.max_rendered_chars
        {
            truncated = true;
            break;
        }

        let kind = classify_line_kind(raw_line, options.show_file_headers);
        if !options.show_file_headers && is_file_header(raw_line) {
            continue;
        }

        rendered_chars = rendered_chars.saturating_add(line_chars);
        out.push(CodeDiffLine {
            kind,
            text: raw_line.to_string(),
        });
    }

    CodeDiffRender {
        lines: out,
        truncated,
    }
}

fn is_file_header(line: &str) -> bool {
    line.starts_with("---") || line.starts_with("+++")
}

fn classify_line_kind(line: &str, show_file_headers: bool) -> CodeDiffLineKind {
    if line.starts_with("@@") {
        return CodeDiffLineKind::Meta;
    }
    if line.starts_with('+') && !line.starts_with("+++") {
        return CodeDiffLineKind::Add;
    }
    if line.starts_with('-') && !line.starts_with("---") {
        return CodeDiffLineKind::Remove;
    }
    if line.starts_with("+++") || line.starts_with("---") {
        if show_file_headers {
            return CodeDiffLineKind::Meta;
        }
        return CodeDiffLineKind::Context;
    }
    CodeDiffLineKind::Context
}

#[cfg(test)]
mod tests {
    use super::{CodeDiffBlock, CodeDiffLineKind, CodeDiffOptions, render_unified_diff};

    #[test]
    fn classifies_file_hunks_and_changes() {
        let block = CodeDiffBlock::new("--- a/a.rs\n+++ b/a.rs\n@@ -1 +1 @@\n-old\n+new\n context");
        let rendered = render_unified_diff(&block, &CodeDiffOptions::default());

        let kinds: Vec<CodeDiffLineKind> = rendered.lines.into_iter().map(|l| l.kind).collect();
        assert_eq!(
            kinds,
            vec![
                CodeDiffLineKind::Meta,
                CodeDiffLineKind::Meta,
                CodeDiffLineKind::Meta,
                CodeDiffLineKind::Remove,
                CodeDiffLineKind::Add,
                CodeDiffLineKind::Context,
            ]
        );
    }

    #[test]
    fn can_hide_file_headers() {
        let block = CodeDiffBlock::new("--- a/a.rs\n+++ b/a.rs\n@@ -1 +1 @@\n-old\n+new");
        let options = CodeDiffOptions {
            show_file_headers: false,
            ..CodeDiffOptions::default()
        };
        let rendered = render_unified_diff(&block, &options);

        assert_eq!(rendered.lines.len(), 3);
        assert_eq!(rendered.lines[0].kind, CodeDiffLineKind::Meta);
        assert_eq!(rendered.lines[1].kind, CodeDiffLineKind::Remove);
        assert_eq!(rendered.lines[2].kind, CodeDiffLineKind::Add);
    }

    #[test]
    fn truncates_by_line_limit() {
        let block = CodeDiffBlock::new("a\nb\nc\nd");
        let options = CodeDiffOptions {
            max_rendered_lines: 2,
            ..CodeDiffOptions::default()
        };

        let rendered = render_unified_diff(&block, &options);
        assert_eq!(rendered.lines.len(), 2);
        assert!(rendered.truncated);
    }

    #[test]
    fn malformed_input_falls_back_without_panic() {
        let block = CodeDiffBlock::new("@@ bad\n+++\n---\n+\n-\n");
        let rendered = render_unified_diff(&block, &CodeDiffOptions::default());
        assert!(!rendered.lines.is_empty());
    }
}
