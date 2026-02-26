use similar::{ChangeTag, TextDiff};

pub struct LineDiff {
    pub added_lines: usize,
    pub removed_lines: usize,
    pub unified: String,
}

pub fn build_unified_line_diff(before: &str, after: &str, path: &str) -> LineDiff {
    let diff = TextDiff::from_lines(before, after);
    let mut added_lines = 0;
    let mut removed_lines = 0;

    for change in diff.iter_all_changes() {
        match change.tag() {
            ChangeTag::Insert => added_lines += 1,
            ChangeTag::Delete => removed_lines += 1,
            ChangeTag::Equal => {}
        }
    }

    let unified = diff
        .unified_diff()
        .context_radius(3)
        .header(&format!("a/{path}"), &format!("b/{path}"))
        .to_string();

    LineDiff {
        added_lines,
        removed_lines,
        unified,
    }
}
