use crate::error::Result;
use crate::index::LineIndex;
use crate::repo::ChunkStorage;

use super::read_chunk_lines;

/// Result of processing a chunk of lines.
pub struct ProcessedChunk {
    /// Chunk index in the original storage.
    pub chunk_idx: usize,
    /// The global line number where this chunk starts.
    pub global_line_start: usize,
    /// The processed lines.
    pub lines: Vec<String>,
}

/// A streaming iterator over lines in a log repository.
/// Reads one chunk at a time to bound memory usage.
///
/// For a 10GB file with 10,000 lines/chunk and ~200 bytes/line,
/// each chunk is ~2MB decompressed. This keeps memory usage at
/// O(chunk_size) instead of O(total_lines).
pub struct LineStream<'a> {
    storage: &'a ChunkStorage,
    index: &'a LineIndex,
    current_chunk: usize,
    total_chunks: usize,
}

impl<'a> LineStream<'a> {
    pub fn new(storage: &'a ChunkStorage, index: &'a LineIndex) -> Self {
        let total_chunks = index.chunks.len();
        Self {
            storage,
            index,
            current_chunk: 0,
            total_chunks,
        }
    }

    /// Read the next chunk of lines.
    /// Returns None when all chunks have been read.
    pub fn next_chunk(&mut self) -> Result<Option<ProcessedChunk>> {
        if self.current_chunk >= self.total_chunks {
            return Ok(None);
        }

        let chunk_idx = self.current_chunk;
        let global_line_start = self.index.chunks[chunk_idx].line_start;
        let lines = read_chunk_lines(self.storage, self.index, chunk_idx)?;

        self.current_chunk += 1;

        Ok(Some(ProcessedChunk {
            chunk_idx,
            global_line_start,
            lines,
        }))
    }

    /// Reset the stream to the beginning.
    pub fn reset(&mut self) {
        self.current_chunk = 0;
    }

    /// Total number of chunks.
    pub fn total_chunks(&self) -> usize {
        self.total_chunks
    }

    /// Number of chunks remaining.
    pub fn remaining_chunks(&self) -> usize {
        self.total_chunks - self.current_chunk
    }
}
