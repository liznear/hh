/// Minimal rectangular render area.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[non_exhaustive]
pub struct Area {
    pub x: u16,
    pub y: u16,
    pub width: u16,
    pub height: u16,
}

/// Shared render context.
#[derive(Debug, Default)]
#[non_exhaustive]
pub struct RenderCtx<'a> {
    pub theme: Option<&'a crate::theme::Theme>,
}

/// Compute height needed to render for a given width.
pub trait Measure {
    fn measure_height(&self, width: u16) -> u16;
}

/// Render into a caller-provided area.
pub trait Render {
    fn render(&self, area: Area, ctx: &RenderCtx<'_>);
}

/// Heterogeneous child node used by containers.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum WidgetNode {
    Markdown(crate::markdown::MarkdownBlock),
    CodeDiff(crate::codediff::CodeDiffBlock),
    Spacer(u16),
}

impl WidgetNode {
    pub fn measured_height(&self, width: u16) -> u16 {
        match self {
            Self::Markdown(block) => {
                let wrap_width = usize::from(width.max(1));
                let lines =
                    crate::markdown::markdown_to_lines_with_indent(&block.source, wrap_width, "");
                u16::try_from(lines.len()).unwrap_or(u16::MAX)
            }
            Self::CodeDiff(block) => {
                if block.unified_diff.is_empty() {
                    0
                } else {
                    u16::try_from(block.unified_diff.lines().count()).unwrap_or(u16::MAX)
                }
            }
            Self::Spacer(height) => *height,
        }
    }
}
