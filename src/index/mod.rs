mod builder;

pub use builder::IndexBuilder;

use serde::{Deserialize, Serialize};

/// Represents a chunk of lines in the log repository.
/// Each chunk contains a contiguous range of lines stored compressed.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChunkInfo {
    /// Chunk sequential ID
    pub id: u32,
    /// First line number in this chunk (0-based)
    pub line_start: usize,
    /// Number of lines in this chunk
    pub line_count: usize,
    /// Byte offset of each line within the decompressed chunk data
    pub line_offsets: Vec<u32>,
}

/// Line index for fast random access to any line in the log.
/// Maps line numbers to (chunk_id, offset_within_chunk).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LineIndex {
    pub total_lines: usize,
    pub chunks: Vec<ChunkInfo>,
    pub lines_per_chunk: usize,
}

impl LineIndex {
    /// Find which chunk contains a given line number.
    /// Returns (chunk_index, line_offset_within_chunk).
    pub fn locate_line(&self, line_num: usize) -> Option<(usize, usize)> {
        if line_num >= self.total_lines {
            return None;
        }
        // Binary search for the chunk containing this line
        let chunk_idx = self
            .chunks
            .partition_point(|c| c.line_start + c.line_count <= line_num)
            .min(self.chunks.len().saturating_sub(1));

        let chunk = &self.chunks[chunk_idx];
        if line_num >= chunk.line_start && line_num < chunk.line_start + chunk.line_count {
            Some((chunk_idx, line_num - chunk.line_start))
        } else {
            None
        }
    }

    /// Get line byte range within a decompressed chunk.
    pub fn line_range_in_chunk(&self, chunk_idx: usize, line_in_chunk: usize) -> (usize, usize) {
        let chunk = &self.chunks[chunk_idx];
        let start = chunk.line_offsets[line_in_chunk] as usize;
        let end = if line_in_chunk + 1 < chunk.line_offsets.len() {
            chunk.line_offsets[line_in_chunk + 1] as usize
        } else {
            // Last line in chunk - end is determined by data length
            // Caller should use chunk data length
            usize::MAX
        };
        (start, end)
    }
}
