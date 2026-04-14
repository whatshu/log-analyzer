pub mod collector;
mod processor;
mod stream;

pub use collector::{CollectResult, Collector};
pub use processor::ChunkedProcessor;
pub use stream::{LineStream, ProcessedChunk};

use crate::error::Result;
use crate::index::LineIndex;
use crate::repo::ChunkStorage;

/// Read lines from a single chunk, stripping trailing newlines.
pub fn read_chunk_lines(
    storage: &ChunkStorage,
    index: &LineIndex,
    chunk_idx: usize,
) -> Result<Vec<String>> {
    let chunk_info = &index.chunks[chunk_idx];
    let chunk_data = storage.read_chunk(chunk_info.id)?;

    let mut lines = Vec::with_capacity(chunk_info.line_count);
    for line_in_chunk in 0..chunk_info.line_count {
        let start = chunk_info.line_offsets[line_in_chunk] as usize;
        let end = if line_in_chunk + 1 < chunk_info.line_offsets.len() {
            chunk_info.line_offsets[line_in_chunk + 1] as usize
        } else {
            chunk_data.len()
        };
        let line = String::from_utf8_lossy(&chunk_data[start..end]);
        lines.push(line.trim_end_matches('\n').to_string());
    }
    Ok(lines)
}
