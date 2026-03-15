use std::ops::Range;

use crate::app::components::messages_layout::BlockLayoutRow;

pub trait MessageBlock {
    fn measured_height(&self, width: u16) -> u16;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TranscriptBlock {
    LegacyMessage { height: u16 },
}

impl MessageBlock for TranscriptBlock {
    fn measured_height(&self, _width: u16) -> u16 {
        match self {
            Self::LegacyMessage { height } => *height,
        }
    }
}

pub fn build_legacy_blocks_from_starts(
    starts: &[usize],
    total_lines: usize,
) -> Vec<TranscriptBlock> {
    starts
        .iter()
        .enumerate()
        .map(|(index, start)| {
            let end = starts.get(index + 1).copied().unwrap_or(total_lines);
            let height = end.saturating_sub(*start).max(1);
            let height = height.min(u16::MAX as usize) as u16;
            TranscriptBlock::LegacyMessage { height }
        })
        .collect()
}

pub fn measured_heights(blocks: &[TranscriptBlock], width: u16) -> Vec<u16> {
    blocks
        .iter()
        .map(|block| block.measured_height(width.max(1)))
        .collect()
}

pub fn visible_message_range(rows: &[BlockLayoutRow], range: Range<usize>) -> Range<usize> {
    let start = rows
        .get(range.start)
        .map(|row| row.block_index)
        .unwrap_or(range.start);
    let end = rows
        .get(range.end)
        .map(|row| row.block_index)
        .unwrap_or(range.end);

    start..end
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_legacy_blocks_maps_message_heights() {
        let blocks = build_legacy_blocks_from_starts(&[0, 2, 5], 6);
        assert_eq!(blocks.len(), 3);
        assert_eq!(blocks[0], TranscriptBlock::LegacyMessage { height: 2 });
        assert_eq!(blocks[1], TranscriptBlock::LegacyMessage { height: 3 });
        assert_eq!(blocks[2], TranscriptBlock::LegacyMessage { height: 1 });
    }

    #[test]
    fn visible_message_range_maps_layout_indexes() {
        let rows = vec![
            BlockLayoutRow {
                block_index: 1,
                start_y: 0,
                height: 2,
            },
            BlockLayoutRow {
                block_index: 2,
                start_y: 2,
                height: 2,
            },
            BlockLayoutRow {
                block_index: 4,
                start_y: 4,
                height: 2,
            },
        ];

        assert_eq!(visible_message_range(&rows, 0..2), 1..4);
    }
}
