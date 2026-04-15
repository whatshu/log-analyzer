use std::io::{BufWriter, Write};
use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use rayon::prelude::*;
use regex::Regex;

use crate::error::Result;
use crate::index::LineIndex;
use crate::repo::ChunkStorage;


/// Progress callback: (chunks_processed, total_chunks)
pub type ProgressCallback = Box<dyn Fn(usize, usize) + Send + Sync>;

/// Chunked parallel processor for large log files.
/// Processes data chunk-by-chunk to avoid loading the entire file into memory.
///
/// Design principles:
/// - Only one chunk (or a small batch) is decompressed at a time per thread
/// - Filter/replace operations emit results as they go
/// - Count operations aggregate without materializing results
pub struct ChunkedProcessor<'a> {
    storage: &'a ChunkStorage,
    index: &'a LineIndex,
}

impl<'a> ChunkedProcessor<'a> {
    pub fn new(storage: &'a ChunkStorage, index: &'a LineIndex) -> Self {
        Self { storage, index }
    }

    /// Count lines matching a regex pattern, chunk-by-chunk in parallel.
    /// Memory usage: O(chunk_size) per thread, not O(total_lines).
    pub fn count_matches(&self, pattern: &str) -> Result<usize> {
        let re = Regex::new(pattern)?;
        let total_chunks = self.index.chunks.len();

        (0..total_chunks)
            .into_par_iter()
            .map(|chunk_idx| {
                let chunk_info = &self.index.chunks[chunk_idx];
                let chunk_data = self.storage.read_chunk(chunk_info.id)?;
                let text = unsafe { std::str::from_utf8_unchecked(&chunk_data) };
                Ok(text.split('\n').filter(|l| !l.is_empty() && re.is_match(l)).count())
            })
            .try_reduce(|| 0, |a, b| Ok(a + b))
    }

    /// Stream-filter lines matching a regex directly to an output file.
    /// Never holds more than one chunk in memory at a time.
    pub fn filter_to_file(
        &self,
        pattern: &str,
        keep: bool,
        output: &Path,
        progress: Option<ProgressCallback>,
    ) -> Result<usize> {
        let re = Regex::new(pattern)?;
        let total_chunks = self.index.chunks.len();
        let mut written = 0usize;

        let file = std::fs::File::create(output)?;
        let mut writer = BufWriter::with_capacity(256 * 1024, file);

        for chunk_idx in 0..total_chunks {
            let chunk_info = &self.index.chunks[chunk_idx];
            let chunk_data = self.storage.read_chunk(chunk_info.id)?;
            let text = unsafe { std::str::from_utf8_unchecked(&chunk_data) };

            for line in text.split('\n') {
                if line.is_empty() {
                    continue;
                }
                if re.is_match(line) == keep {
                    writer.write_all(line.as_bytes())?;
                    writer.write_all(b"\n")?;
                    written += 1;
                }
            }

            if let Some(ref cb) = progress {
                cb(chunk_idx + 1, total_chunks);
            }
        }

        writer.flush()?;
        Ok(written)
    }

    /// Stream-replace matching text and write directly to an output file.
    pub fn replace_to_file(
        &self,
        pattern: &str,
        replacement: &str,
        output: &Path,
        progress: Option<ProgressCallback>,
    ) -> Result<usize> {
        let re = Regex::new(pattern)?;
        let total_chunks = self.index.chunks.len();
        let replacement = replacement.to_string();
        let modified_count = Arc::new(AtomicUsize::new(0));

        let file = std::fs::File::create(output)?;
        let mut writer = BufWriter::with_capacity(256 * 1024, file);

        for chunk_idx in 0..total_chunks {
            let chunk_info = &self.index.chunks[chunk_idx];
            let chunk_data = self.storage.read_chunk(chunk_info.id)?;
            let text = unsafe { std::str::from_utf8_unchecked(&chunk_data) };

            for line in text.split('\n') {
                if line.is_empty() {
                    continue;
                }
                let replaced = re.replace_all(line, replacement.as_str());
                if replaced != line {
                    modified_count.fetch_add(1, Ordering::Relaxed);
                }
                writer.write_all(replaced.as_bytes())?;
                writer.write_all(b"\n")?;
            }

            if let Some(ref cb) = progress {
                cb(chunk_idx + 1, total_chunks);
            }
        }

        writer.flush()?;
        Ok(modified_count.load(Ordering::Relaxed))
    }

    /// Search for lines matching a pattern with context, streaming results.
    /// Returns (line_number, line_content) pairs without loading all data.
    pub fn search(
        &self,
        pattern: &str,
        max_results: usize,
    ) -> Result<Vec<(usize, String)>> {
        let re = Regex::new(pattern)?;
        let total_chunks = self.index.chunks.len();
        let mut results = Vec::new();

        for chunk_idx in 0..total_chunks {
            let chunk_info = &self.index.chunks[chunk_idx];
            let chunk_data = self.storage.read_chunk(chunk_info.id)?;
            let text = unsafe { std::str::from_utf8_unchecked(&chunk_data) };

            let mut line_in_chunk = 0usize;
            for line in text.split('\n') {
                if line.is_empty() {
                    continue;
                }
                if re.is_match(line) {
                    let global_line = chunk_info.line_start + line_in_chunk;
                    results.push((global_line, line.to_string()));
                    if results.len() >= max_results {
                        return Ok(results);
                    }
                }
                line_in_chunk += 1;
            }
        }

        Ok(results)
    }

    /// Parallel search across all chunks.
    /// Faster than sequential search but doesn't guarantee order.
    /// Results are sorted by line number before returning.
    pub fn parallel_search(
        &self,
        pattern: &str,
        max_results: usize,
    ) -> Result<Vec<(usize, String)>> {
        let re = Regex::new(pattern)?;
        let total_chunks = self.index.chunks.len();
        let found_count = Arc::new(AtomicUsize::new(0));

        let chunk_results: Vec<Result<Vec<(usize, String)>>> = (0..total_chunks)
            .into_par_iter()
            .map(|chunk_idx| {
                // Early exit if we already have enough results
                if found_count.load(Ordering::Relaxed) >= max_results {
                    return Ok(Vec::new());
                }

                let chunk_info = &self.index.chunks[chunk_idx];
                let chunk_data = self.storage.read_chunk(chunk_info.id)?;
                let text = unsafe { std::str::from_utf8_unchecked(&chunk_data) };

                let mut matches = Vec::new();
                let mut line_in_chunk = 0usize;
                for line in text.split('\n') {
                    if line.is_empty() {
                        continue;
                    }
                    if re.is_match(line) {
                        let global_line = chunk_info.line_start + line_in_chunk;
                        matches.push((global_line, line.to_string()));
                        found_count.fetch_add(1, Ordering::Relaxed);
                    }
                    line_in_chunk += 1;
                }
                Ok(matches)
            })
            .collect();

        let mut all_results = Vec::new();
        for chunk_result in chunk_results {
            all_results.extend(chunk_result?);
        }

        all_results.sort_by_key(|(line_num, _)| *line_num);
        all_results.truncate(max_results);
        Ok(all_results)
    }

    /// Export the entire log to a file, streaming chunk by chunk.
    pub fn export_to_file(
        &self,
        output: &Path,
        progress: Option<ProgressCallback>,
    ) -> Result<usize> {
        let total_chunks = self.index.chunks.len();
        let mut total_lines = 0usize;

        let file = std::fs::File::create(output)?;
        let mut writer = BufWriter::with_capacity(256 * 1024, file);

        for chunk_idx in 0..total_chunks {
            let chunk_info = &self.index.chunks[chunk_idx];
            let chunk_data = self.storage.read_chunk(chunk_info.id)?;
            // Write raw chunk data directly — it already has newlines
            writer.write_all(&chunk_data)?;
            total_lines += chunk_info.line_count;

            if let Some(ref cb) = progress {
                cb(chunk_idx + 1, total_chunks);
            }
        }

        writer.flush()?;
        Ok(total_lines)
    }

    /// Compute statistics over the log without loading everything.
    pub fn stats(&self) -> Result<LogStats> {
        let total_chunks = self.index.chunks.len();

        let chunk_stats: Vec<Result<(usize, usize, usize)>> = (0..total_chunks)
            .into_par_iter()
            .map(|chunk_idx| {
                let chunk_info = &self.index.chunks[chunk_idx];
                let chunk_data = self.storage.read_chunk(chunk_info.id)?;
                let text = unsafe { std::str::from_utf8_unchecked(&chunk_data) };
                let mut total_bytes = 0usize;
                let mut max_len = 0usize;
                let mut min_len = usize::MAX;
                for line in text.split('\n') {
                    if line.is_empty() {
                        continue;
                    }
                    let len = line.len();
                    total_bytes += len;
                    if len > max_len { max_len = len; }
                    if len < min_len { min_len = len; }
                }
                if min_len == usize::MAX { min_len = 0; }
                Ok((total_bytes, max_len, min_len))
            })
            .collect();

        let mut total_bytes = 0;
        let mut max_line_len = 0;
        let mut min_line_len = usize::MAX;

        for stat in chunk_stats {
            let (bytes, max_l, min_l) = stat?;
            total_bytes += bytes;
            max_line_len = max_line_len.max(max_l);
            min_line_len = min_line_len.min(min_l);
        }

        let total_lines = self.index.total_lines;
        if total_lines == 0 {
            min_line_len = 0;
        }
        let avg_line_len = if total_lines > 0 {
            total_bytes as f64 / total_lines as f64
        } else {
            0.0
        };

        Ok(LogStats {
            total_lines,
            total_bytes,
            avg_line_len,
            max_line_len,
            min_line_len,
            chunk_count: total_chunks,
        })
    }
}

/// Statistics about a log repository, computed in streaming fashion.
#[derive(Debug, Clone)]
pub struct LogStats {
    pub total_lines: usize,
    pub total_bytes: usize,
    pub avg_line_len: f64,
    pub max_line_len: usize,
    pub min_line_len: usize,
    pub chunk_count: usize,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::index::IndexBuilder;
    use crate::repo::ChunkStorage;
    use tempfile::TempDir;

    fn setup_test_repo(lines: &[&str]) -> (TempDir, ChunkStorage, LineIndex) {
        let tmp = TempDir::new().unwrap();
        let chunks_dir = tmp.path().join("chunks");
        std::fs::create_dir_all(&chunks_dir).unwrap();

        let data = lines.join("\n") + "\n";
        let builder = IndexBuilder::new().with_lines_per_chunk(3);
        let (index, chunks_data) = builder.build(data.as_bytes());

        let storage = ChunkStorage::new(chunks_dir);
        storage.write_chunks(&chunks_data).unwrap();

        (tmp, storage, index)
    }

    #[test]
    fn test_count_matches() {
        let lines: Vec<&str> = (0..10)
            .map(|i| if i % 2 == 0 { "ERROR: fail" } else { "INFO: ok" })
            .collect();
        let (_tmp, storage, index) = setup_test_repo(&lines);

        let processor = ChunkedProcessor::new(&storage, &index);
        let count = processor.count_matches("ERROR").unwrap();
        assert_eq!(count, 5);
    }

    #[test]
    fn test_search() {
        let lines = vec![
            "INFO: start",
            "ERROR: disk full",
            "INFO: retry",
            "ERROR: timeout",
            "INFO: done",
        ];
        let (_tmp, storage, index) = setup_test_repo(&lines);

        let processor = ChunkedProcessor::new(&storage, &index);
        let results = processor.search("ERROR", 10).unwrap();
        assert_eq!(results.len(), 2);
        assert_eq!(results[0].0, 1);
        assert_eq!(results[1].0, 3);
    }

    #[test]
    fn test_parallel_search() {
        let lines: Vec<&str> = (0..20)
            .map(|i| {
                if i % 5 == 0 {
                    "ERROR: problem"
                } else {
                    "INFO: ok"
                }
            })
            .collect();
        let (_tmp, storage, index) = setup_test_repo(&lines);

        let processor = ChunkedProcessor::new(&storage, &index);
        let results = processor.parallel_search("ERROR", 100).unwrap();
        assert_eq!(results.len(), 4);
        // Results should be sorted by line number
        for window in results.windows(2) {
            assert!(window[0].0 < window[1].0);
        }
    }

    #[test]
    fn test_filter_to_file() {
        let lines = vec!["keep_a", "drop_b", "keep_c", "drop_d", "keep_e"];
        let (_tmp, storage, index) = setup_test_repo(&lines);

        let output = _tmp.path().join("filtered.txt");
        let processor = ChunkedProcessor::new(&storage, &index);
        let count = processor
            .filter_to_file("keep", true, &output, None)
            .unwrap();

        assert_eq!(count, 3);
        let content = std::fs::read_to_string(&output).unwrap();
        let result_lines: Vec<&str> = content.trim().split('\n').collect();
        assert_eq!(result_lines, vec!["keep_a", "keep_c", "keep_e"]);
    }

    #[test]
    fn test_replace_to_file() {
        let lines = vec!["hello world", "foo bar", "hello foo"];
        let (_tmp, storage, index) = setup_test_repo(&lines);

        let output = _tmp.path().join("replaced.txt");
        let processor = ChunkedProcessor::new(&storage, &index);
        let modified = processor
            .replace_to_file("hello", "HI", &output, None)
            .unwrap();

        assert_eq!(modified, 2);
        let content = std::fs::read_to_string(&output).unwrap();
        let result_lines: Vec<&str> = content.trim().split('\n').collect();
        assert_eq!(result_lines, vec!["HI world", "foo bar", "HI foo"]);
    }

    #[test]
    fn test_export_to_file() {
        let lines = vec!["line1", "line2", "line3"];
        let (_tmp, storage, index) = setup_test_repo(&lines);

        let output = _tmp.path().join("export.txt");
        let processor = ChunkedProcessor::new(&storage, &index);
        let count = processor.export_to_file(&output, None).unwrap();

        assert_eq!(count, 3);
        let content = std::fs::read_to_string(&output).unwrap();
        assert_eq!(content, "line1\nline2\nline3\n");
    }

    #[test]
    fn test_stats() {
        let lines = vec!["short", "a medium length line", "x"];
        let (_tmp, storage, index) = setup_test_repo(&lines);

        let processor = ChunkedProcessor::new(&storage, &index);
        let stats = processor.stats().unwrap();

        assert_eq!(stats.total_lines, 3);
        assert_eq!(stats.max_line_len, 20);
        assert_eq!(stats.min_line_len, 1);
        assert_eq!(stats.chunk_count, 1); // 3 lines, 3 per chunk = 1 chunk
    }

    #[test]
    fn test_filter_with_progress() {
        let lines = vec!["a", "b", "c", "d", "e", "f", "g"];
        let (_tmp, storage, index) = setup_test_repo(&lines);

        let output = _tmp.path().join("prog.txt");
        let progress_count = Arc::new(AtomicUsize::new(0));
        let pc = progress_count.clone();
        let total_chunks = index.chunks.len();

        let processor = ChunkedProcessor::new(&storage, &index);
        processor
            .filter_to_file(
                ".",
                true,
                &output,
                Some(Box::new(move |done, total| {
                    pc.fetch_add(1, Ordering::Relaxed);
                    assert_eq!(total, total_chunks);
                    assert!(done <= total);
                })),
            )
            .unwrap();

        assert_eq!(progress_count.load(Ordering::Relaxed), total_chunks);
    }
}
