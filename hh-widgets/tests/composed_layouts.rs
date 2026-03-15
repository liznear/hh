use hh_widgets::codediff::{CodeDiffBlock, CodeDiffLineKind, CodeDiffOptions, render_unified_diff};
use hh_widgets::markdown::markdown_to_lines_with_indent;
use hh_widgets::popup::{Anchor, PopupOptions, PopupRequest, popup_from_request};
use hh_widgets::scrollable::{ScrollableState, measure_children, visible_range};
use hh_widgets::widget::{Area, WidgetNode};

#[test]
fn composed_layout_markdown_diff_scroll_and_popup_is_deterministic() {
    let children = vec![
        WidgetNode::Markdown(hh_widgets::markdown::MarkdownBlock::new(
            "# Title\n\n- one\n- two",
        )),
        WidgetNode::Spacer(1),
        WidgetNode::CodeDiff(CodeDiffBlock::new(
            "--- a/src/lib.rs\n+++ b/src/lib.rs\n@@ -1 +1 @@\n-old\n+new",
        )),
    ];

    let layout_a = measure_children(&children, 72);
    let layout_b = measure_children(&children, 72);
    assert_eq!(layout_a, layout_b);

    let mut state = ScrollableState::default();
    state.viewport_height = 8;
    let visible_a = visible_range(&layout_a, &state);
    let visible_b = visible_range(&layout_b, &state);
    assert_eq!(visible_a, visible_b);

    let md_a = markdown_to_lines_with_indent("alpha\nbeta", 40, "  ");
    let md_b = markdown_to_lines_with_indent("alpha\nbeta", 40, "  ");
    assert_eq!(md_a, md_b);

    let mut popup_options = PopupOptions::default();
    popup_options.anchor = Anchor::BottomLeft;
    let mut req = PopupRequest::default();
    req.anchor_x = 4;
    req.anchor_y = 20;
    req.width = 30;
    req.height = 6;
    req.options = popup_options;

    let mut viewport = Area::default();
    viewport.width = 80;
    viewport.height = 24;
    let popup_a = popup_from_request(req.clone(), viewport);
    let popup_b = popup_from_request(req, viewport);
    assert_eq!(popup_a, popup_b);
}

#[test]
fn codediff_composed_layout_snapshot_shape_is_stable() {
    let diff = CodeDiffBlock::new("--- a/a.rs\n+++ b/a.rs\n@@ -1 +1 @@\n-old\n+new\n context");
    let rendered = render_unified_diff(&diff, &CodeDiffOptions::default());

    let summary = rendered
        .lines
        .iter()
        .map(|line| match line.kind {
            CodeDiffLineKind::Meta => "M",
            CodeDiffLineKind::Remove => "R",
            CodeDiffLineKind::Add => "A",
            CodeDiffLineKind::Context => "C",
        })
        .collect::<Vec<_>>()
        .join("");

    assert_eq!(summary, "MMMRAC");
    assert!(!rendered.truncated);
}

#[test]
fn widgets_work_without_hh_runtime_types() {
    let children = vec![
        WidgetNode::Markdown(hh_widgets::markdown::MarkdownBlock::new("hello")),
        WidgetNode::CodeDiff(CodeDiffBlock::new("+new")),
    ];
    let layout = measure_children(&children, 50);
    let mut state = ScrollableState::default();
    state.viewport_height = 3;
    let range = visible_range(&layout, &state);

    assert!(range.end >= range.start);
}
