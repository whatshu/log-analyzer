use rayon::prelude::*;

use super::{ChunkInfo, LineIndex};

/// Default number of lines per chunk.
/// 10000 lines per chunk provides good balance between
/// compression ratio and random access speed.
pub const DEFAULT_LINES_PER_CHUNK: usize = 10_000;

pub struct IndexBuilder {
    lines_per_chunk: usize,
}

impl IndexBuilder {
    pub fn new() -> Self {
        Self {
            lines_per_chunk: DEFAULT_LINES_PER_CHUNK,
        }
    }

    pub fn with_lines_per_chunk(mut self, n: usize) -> Self {
        self.lines_per_chunk = n;
        self
    }

    /// Build a line index from raw data.
    /// Returns the index and the chunked data (each chunk is a Vec<u8> of raw line data).
    pub fn build(&self, data: &[u8]) -> (LineIndex, Vec<Vec<u8>>) {
        // First pass: find all newline positions in parallel
        let line_positions = self.find_line_boundaries(data);
        let total_lines = line_positions.len();

        // Split into chunks
        let chunk_count = (total_lines + self.lines_per_chunk - 1) / self.lines_per_chunk;
        let mut chunks_info = Vec::with_capacity(chunk_count);
        let mut chunks_data = Vec::with_capacity(chunk_count);

        for (chunk_id, chunk_lines) in line_positions.chunks(self.lines_per_chunk).enumerate() {
            let line_start = chunk_id * self.lines_per_chunk;
            let line_count = chunk_lines.len();

            // Calculate chunk data boundaries
            let chunk_data_start = chunk_lines[0].0;
            let chunk_data_end = chunk_lines[line_count - 1].1;
            let chunk_data = data[chunk_data_start..chunk_data_end].to_vec();

            // Line offsets relative to chunk start
            let line_offsets: Vec<u32> = chunk_lines
                .iter()
                .map(|(start, _)| (*start - chunk_data_start) as u32)
                .collect();

            chunks_info.push(ChunkInfo {
                id: chunk_id as u32,
                line_start,
                line_count,
                line_offsets,
            });
            chunks_data.push(chunk_data);
        }

        let index = LineIndex {
            total_lines,
            chunks: chunks_info,
            lines_per_chunk: self.lines_per_chunk,
        };

        (index, chunks_data)
    }

    /// Find all line boundaries (start, end) byte offsets.
    /// Uses parallel scanning for large files.
    fn find_line_boundaries(&self, data: &[u8]) -> Vec<(usize, usize)> {
        if data.is_empty() {
            return Vec::new();
        }

        const SCAN_CHUNK_SIZE: usize = 4 * 1024 * 1024; // 4MB scan chunks

        if data.len() < SCAN_CHUNK_SIZE * 2 {
            // Small data: sequential scan
            return self.scan_lines_sequential(data);
        }

        // Large data: parallel scan to find newline positions, then merge
        let newline_positions: Vec<Vec<usize>> = data
            .par_chunks(SCAN_CHUNK_SIZE)
            .enumerate()
            .map(|(chunk_idx, chunk)| {
                let base_offset = chunk_idx * SCAN_CHUNK_SIZE;
                chunk
                    .iter()
                    .enumerate()
                    .filter(|(_, &b)| b == b'\n')
                    .map(|(i, _)| base_offset + i)
                    .collect()
            })
            .collect();

        // Merge and build line boundaries
        let all_newlines: Vec<usize> = newline_positions.into_iter().flatten().collect();

        let mut lines = Vec::with_capacity(all_newlines.len() + 1);
        let mut line_start = 0;

        for &nl_pos in &all_newlines {
            lines.push((line_start, nl_pos + 1));
            line_start = nl_pos + 1;
        }

        // Handle last line without trailing newline
        if line_start < data.len() {
            lines.push((line_start, data.len()));
        }

        lines
    }

    fn scan_lines_sequential(&self, data: &[u8]) -> Vec<(usize, usize)> {
        let mut lines = Vec::new();
        let mut line_start = 0;

        for (i, &b) in data.iter().enumerate() {
            if b == b'\n' {
                lines.push((line_start, i + 1));
                line_start = i + 1;
            }
        }

        if line_start < data.len() {
            lines.push((line_start, data.len()));
        }

        lines
    }
}

impl Default for IndexBuilder {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_empty_data() {
        let builder = IndexBuilder::new();
        let (index, chunks) = builder.build(b"");
        assert_eq!(index.total_lines, 0);
        assert!(chunks.is_empty());
    }

    #[test]
    fn test_single_line_no_newline() {
        let builder = IndexBuilder::new();
        let (index, chunks) = builder.build(b"hello");
        assert_eq!(index.total_lines, 1);
        assert_eq!(chunks.len(), 1);
        assert_eq!(&chunks[0], b"hello");
    }

    #[test]
    fn test_multiple_lines() {
        let builder = IndexBuilder::with_lines_per_chunk(IndexBuilder::new(), 2);
        let data = b"line1\nline2\nline3\n";
        let (index, chunks) = builder.build(data);
        assert_eq!(index.total_lines, 3);
        assert_eq!(chunks.len(), 2); // 2 lines + 1 line = 2 chunks
    }

    #[test]
    fn test_locate_line() {
        let builder = IndexBuilder::new().with_lines_per_chunk(3);
        let data = b"a\nb\nc\nd\ne\n";
        let (index, _) = builder.build(data);
        assert_eq!(index.total_lines, 5);

        assert_eq!(index.locate_line(0), Some((0, 0)));
        assert_eq!(index.locate_line(2), Some((0, 2)));
        assert_eq!(index.locate_line(3), Some((1, 0)));
        assert_eq!(index.locate_line(4), Some((1, 1)));
        assert_eq!(index.locate_line(5), None);
    }
}
