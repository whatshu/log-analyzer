//! Fast path using ripgrep's grep-searcher for pattern matching.
//!
//! Uses SIMD-accelerated literal optimizations from the ripgrep ecosystem
//! for searching through raw (uncompressed) data and decompressed chunks.

use std::io::Cursor;
use std::path::Path;

use grep_regex::RegexMatcher;
use grep_searcher::sinks::UTF8;
use grep_searcher::SearcherBuilder;

use crate::error::{LogAnalyzerError, Result};
use crate::index::LineIndex;
use crate::repo::ChunkStorage;

/// Count matching lines in a file on disk using ripgrep's searcher.
/// This is the fastest path — works directly on the original file
/// before or independently of repository import.
pub fn count_file_matches(path: &Path, pattern: &str) -> Result<usize> {
    let matcher = RegexMatcher::new(pattern)
        .map_err(|e| LogAnalyzerError::Regex(regex::Error::Syntax(e.to_string())))?;

    let mut count = 0usize;
    let mut searcher = SearcherBuilder::new().line_number(true).build();

    searcher
        .search_path(
            &matcher,
            path,
            UTF8(|_, _| {
                count += 1;
                Ok(true)
            }),
        )
        .map_err(|e| LogAnalyzerError::Repo(format!("search error: {}", e)))?;

    Ok(count)
}

/// Search a file on disk for matching lines, returning (line_number, content).
/// Uses ripgrep's searcher for maximum speed.
pub fn search_file(
    path: &Path,
    pattern: &str,
    max_results: usize,
) -> Result<Vec<(u64, String)>> {
    let matcher = RegexMatcher::new(pattern)
        .map_err(|e| LogAnalyzerError::Regex(regex::Error::Syntax(e.to_string())))?;

    let mut results = Vec::new();
    let mut searcher = SearcherBuilder::new().line_number(true).build();

    searcher
        .search_path(
            &matcher,
            path,
            UTF8(|lnum, line| {
                results.push((lnum, line.trim_end_matches('\n').to_string()));
                Ok(results.len() < max_results)
            }),
        )
        .map_err(|e| LogAnalyzerError::Repo(format!("search error: {}", e)))?;

    Ok(results)
}

/// Count matching lines across compressed chunks using ripgrep's searcher.
/// Each chunk is decompressed and searched with grep-searcher's SIMD path.
pub fn count_chunk_matches(
    storage: &ChunkStorage,
    index: &LineIndex,
    pattern: &str,
) -> Result<usize> {
    let matcher = RegexMatcher::new(pattern)
        .map_err(|e| LogAnalyzerError::Regex(regex::Error::Syntax(e.to_string())))?;

    let total_chunks = index.chunks.len();

    // Process chunks in parallel using rayon, but search each chunk with grep-searcher
    use rayon::prelude::*;
    let results: Vec<Result<usize>> = (0..total_chunks)
        .into_par_iter()
        .map(|chunk_idx| {
            let chunk_info = &index.chunks[chunk_idx];
            let chunk_data = storage.read_chunk(chunk_info.id)?;

            let mut count = 0usize;
            let mut searcher = SearcherBuilder::new().line_number(true).build();

            searcher
                .search_reader(
                    &matcher,
                    Cursor::new(&chunk_data),
                    UTF8(|_, _| {
                        count += 1;
                        Ok(true)
                    }),
                )
                .map_err(|e| LogAnalyzerError::Repo(format!("search error: {}", e)))?;

            Ok(count)
        })
        .collect();

    let mut total = 0;
    for r in results {
        total += r?;
    }
    Ok(total)
}

/// Search compressed chunks for matching lines using grep-searcher.
/// Returns (global_line_number, content) pairs.
pub fn search_chunks(
    storage: &ChunkStorage,
    index: &LineIndex,
    pattern: &str,
    max_results: usize,
) -> Result<Vec<(usize, String)>> {
    let matcher = RegexMatcher::new(pattern)
        .map_err(|e| LogAnalyzerError::Regex(regex::Error::Syntax(e.to_string())))?;

    let total_chunks = index.chunks.len();
    let mut results = Vec::new();

    for chunk_idx in 0..total_chunks {
        let chunk_info = &index.chunks[chunk_idx];
        let chunk_data = storage.read_chunk(chunk_info.id)?;

        let mut searcher = SearcherBuilder::new().line_number(true).build();

        searcher
            .search_reader(
                &matcher,
                Cursor::new(&chunk_data),
                UTF8(|lnum, line| {
                    let global_line = chunk_info.line_start + (lnum as usize - 1);
                    results.push((global_line, line.trim_end_matches('\n').to_string()));
                    Ok(results.len() < max_results)
                }),
            )
            .map_err(|e| LogAnalyzerError::Repo(format!("search error: {}", e)))?;

        if results.len() >= max_results {
            break;
        }
    }

    Ok(results)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::index::IndexBuilder;
    use std::io::Write;
    use tempfile::{NamedTempFile, TempDir};

    #[test]
    fn test_count_file_matches() {
        let mut f = NamedTempFile::new().unwrap();
        writeln!(f, "INFO ok").unwrap();
        writeln!(f, "ERROR fail").unwrap();
        writeln!(f, "INFO ok").unwrap();
        writeln!(f, "ERROR boom").unwrap();
        f.flush().unwrap();

        let count = count_file_matches(f.path(), "ERROR").unwrap();
        assert_eq!(count, 2);
    }

    #[test]
    fn test_search_file() {
        let mut f = NamedTempFile::new().unwrap();
        writeln!(f, "aaa").unwrap();
        writeln!(f, "bbb").unwrap();
        writeln!(f, "aaa").unwrap();
        f.flush().unwrap();

        let results = search_file(f.path(), "aaa", 10).unwrap();
        assert_eq!(results.len(), 2);
        assert_eq!(results[0].0, 1);
        assert_eq!(results[1].0, 3);
    }

    #[test]
    fn test_search_file_limit() {
        let mut f = NamedTempFile::new().unwrap();
        for _ in 0..100 {
            writeln!(f, "match").unwrap();
        }
        f.flush().unwrap();

        let results = search_file(f.path(), "match", 5).unwrap();
        assert_eq!(results.len(), 5);
    }

    fn setup_chunks(lines: &[&str]) -> (TempDir, ChunkStorage, LineIndex) {
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
    fn test_count_chunk_matches() {
        let lines = vec![
            "INFO ok", "ERROR fail", "INFO ok", "ERROR boom", "DEBUG trace",
        ];
        let (_tmp, storage, index) = setup_chunks(&lines);

        let count = count_chunk_matches(&storage, &index, "ERROR").unwrap();
        assert_eq!(count, 2);
    }

    #[test]
    fn test_search_chunks() {
        let lines = vec![
            "aaa", "bbb", "aaa", "ccc", "aaa",
        ];
        let (_tmp, storage, index) = setup_chunks(&lines);

        let results = search_chunks(&storage, &index, "aaa", 10).unwrap();
        assert_eq!(results.len(), 3);
        assert_eq!(results[0].0, 0);
        assert_eq!(results[1].0, 2);
        assert_eq!(results[2].0, 4);
    }

    #[test]
    fn test_search_chunks_limit() {
        let lines: Vec<&str> = (0..20).map(|_| "match").collect();
        let (_tmp, storage, index) = setup_chunks(&lines);

        let results = search_chunks(&storage, &index, "match", 5).unwrap();
        assert_eq!(results.len(), 5);
    }
}
