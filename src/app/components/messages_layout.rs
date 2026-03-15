use std::ops::Range;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BlockLayoutRow {
    pub block_index: usize,
    pub start_y: u32,
    pub height: u16,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BlockLayout {
    pub rows: Vec<BlockLayoutRow>,
    pub visible_range: Range<usize>,
    pub total_height: u32,
}

pub fn compute_layout(
    heights: &[u16],
    scroll_offset: usize,
    viewport_height: usize,
) -> BlockLayout {
    let mut rows = Vec::with_capacity(heights.len());
    let mut y = 0u32;

    for (index, &height) in heights.iter().enumerate() {
        rows.push(BlockLayoutRow {
            block_index: index,
            start_y: y,
            height,
        });
        y = y.saturating_add(u32::from(height));
    }

    let viewport_start = scroll_offset as u32;
    let viewport_end = viewport_start.saturating_add(viewport_height as u32);

    let start = rows
        .iter()
        .position(|row| row.start_y.saturating_add(u32::from(row.height)) > viewport_start)
        .unwrap_or(rows.len());

    let end = rows[start..]
        .iter()
        .position(|row| row.start_y >= viewport_end)
        .map(|idx| start + idx)
        .unwrap_or(rows.len());

    BlockLayout {
        rows,
        visible_range: start..end,
        total_height: y,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_layout_has_empty_visible_range() {
        let layout = compute_layout(&[], 0, 10);
        assert!(layout.rows.is_empty());
        assert_eq!(layout.visible_range, 0..0);
        assert_eq!(layout.total_height, 0);
    }

    #[test]
    fn computes_visible_range_for_mixed_heights() {
        let layout = compute_layout(&[2, 3, 1, 4], 3, 3);
        assert_eq!(layout.rows.len(), 4);
        assert_eq!(layout.rows[0].start_y, 0);
        assert_eq!(layout.rows[1].start_y, 2);
        assert_eq!(layout.rows[2].start_y, 5);
        assert_eq!(layout.rows[3].start_y, 6);
        assert_eq!(layout.visible_range, 1..3);
        assert_eq!(layout.total_height, 10);
    }

    #[test]
    fn offset_beyond_total_height_yields_empty_range() {
        let layout = compute_layout(&[2, 2], 20, 5);
        assert_eq!(layout.visible_range, 2..2);
        assert_eq!(layout.total_height, 4);
    }
}
