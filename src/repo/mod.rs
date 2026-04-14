mod metadata;
mod storage;

pub use metadata::RepoMetadata;
pub use storage::ChunkStorage;

use std::fs;
use std::path::{Path, PathBuf};

use crate::engine::{ChunkedProcessor, LineStream};
use crate::error::{LogAnalyzerError, Result};
use crate::index::{IndexBuilder, LineIndex};
use crate::operator::{Operation, OperationRecord};

/// A log repository that stores compressed log data with operation history.
///
/// Directory layout:
/// ```text
/// <repo_path>/
/// ├── meta.json           # Repository metadata
/// ├── index.json          # Line index
/// ├── chunks/             # Compressed data chunks
/// │   ├── 000000.zst
/// │   ├── 000001.zst
/// │   └── ...
/// ├── operations.json     # Operation journal
/// └── snapshots/          # Materialized snapshots after operations
///     └── ...
/// ```
pub struct LogRepo {
    path: PathBuf,
    pub metadata: RepoMetadata,
    pub index: LineIndex,
    storage: ChunkStorage,
    operations: Vec<OperationRecord>,
    /// Cached current state lines (after all operations applied).
    /// None means we need to recompute from original + operations.
    current_lines: Option<Vec<String>>,
}

impl LogRepo {
    /// Import a text file into a new log repository.
    pub fn import(repo_path: &Path, source_file: &Path) -> Result<Self> {
        if repo_path.exists() {
            return Err(LogAnalyzerError::Repo(format!(
                "Repository already exists: {}",
                repo_path.display()
            )));
        }

        // Read source file
        let data = fs::read(source_file)?;

        Self::import_from_bytes(repo_path, &data, source_file.to_string_lossy().to_string())
    }

    /// Import raw bytes into a new log repository.
    pub fn import_from_bytes(
        repo_path: &Path,
        data: &[u8],
        source_name: String,
    ) -> Result<Self> {
        // Create directory structure
        fs::create_dir_all(repo_path)?;
        fs::create_dir_all(repo_path.join("chunks"))?;
        fs::create_dir_all(repo_path.join("snapshots"))?;

        // Build line index and chunk data
        let builder = IndexBuilder::new();
        let (index, chunks_data) = builder.build(data);

        // Create chunk storage and write compressed chunks
        let storage = ChunkStorage::new(repo_path.join("chunks"));
        storage.write_chunks(&chunks_data)?;

        // Create metadata
        let metadata = RepoMetadata::new(
            source_name,
            data.len() as u64,
            index.total_lines,
        );

        // Save index and metadata
        let index_json = serde_json::to_string_pretty(&index)?;
        fs::write(repo_path.join("index.json"), index_json)?;

        let meta_json = serde_json::to_string_pretty(&metadata)?;
        fs::write(repo_path.join("meta.json"), meta_json)?;

        // Empty operations journal
        fs::write(repo_path.join("operations.json"), "[]")?;

        Ok(Self {
            path: repo_path.to_path_buf(),
            metadata,
            index,
            storage,
            operations: Vec::new(),
            current_lines: None,
        })
    }

    /// Open an existing log repository.
    pub fn open(repo_path: &Path) -> Result<Self> {
        if !repo_path.exists() {
            return Err(LogAnalyzerError::Repo(format!(
                "Repository not found: {}",
                repo_path.display()
            )));
        }

        let metadata: RepoMetadata =
            serde_json::from_str(&fs::read_to_string(repo_path.join("meta.json"))?)?;

        let index: LineIndex =
            serde_json::from_str(&fs::read_to_string(repo_path.join("index.json"))?)?;

        let operations: Vec<OperationRecord> =
            serde_json::from_str(&fs::read_to_string(repo_path.join("operations.json"))?)?;

        let storage = ChunkStorage::new(repo_path.join("chunks"));

        Ok(Self {
            path: repo_path.to_path_buf(),
            metadata,
            index,
            storage,
            operations,
            current_lines: None,
        })
    }

    /// Clone this repository to a new path.
    pub fn clone_to(&self, dest_path: &Path) -> Result<Self> {
        if dest_path.exists() {
            return Err(LogAnalyzerError::Repo(format!(
                "Destination already exists: {}",
                dest_path.display()
            )));
        }
        // Copy the entire directory tree
        copy_dir_all(&self.path, dest_path)?;
        Self::open(dest_path)
    }

    /// Read a single line from the original (unmodified) data.
    pub fn read_original_line(&self, line_num: usize) -> Result<String> {
        if line_num >= self.index.total_lines {
            return Err(LogAnalyzerError::LineOutOfRange(
                line_num,
                self.index.total_lines,
            ));
        }

        let (chunk_idx, line_in_chunk) = self
            .index
            .locate_line(line_num)
            .ok_or(LogAnalyzerError::LineOutOfRange(
                line_num,
                self.index.total_lines,
            ))?;

        let chunk_data = self.storage.read_chunk(chunk_idx as u32)?;
        let (start, end) = self.index.line_range_in_chunk(chunk_idx, line_in_chunk);
        let actual_end = end.min(chunk_data.len());

        let line = String::from_utf8_lossy(&chunk_data[start..actual_end]);
        // Strip trailing newline
        Ok(line.trim_end_matches('\n').to_string())
    }

    /// Read a range of lines from original data.
    pub fn read_original_lines(&self, start: usize, count: usize) -> Result<Vec<String>> {
        let end = (start + count).min(self.index.total_lines);
        let mut lines = Vec::with_capacity(end - start);

        // Group by chunk for efficiency
        let mut current_chunk_idx: Option<usize> = None;
        let mut current_chunk_data: Vec<u8> = Vec::new();

        for line_num in start..end {
            let (chunk_idx, line_in_chunk) = self
                .index
                .locate_line(line_num)
                .ok_or(LogAnalyzerError::LineOutOfRange(
                    line_num,
                    self.index.total_lines,
                ))?;

            // Load chunk if needed
            if current_chunk_idx != Some(chunk_idx) {
                current_chunk_data = self.storage.read_chunk(chunk_idx as u32)?;
                current_chunk_idx = Some(chunk_idx);
            }

            let (start_byte, end_byte) =
                self.index.line_range_in_chunk(chunk_idx, line_in_chunk);
            let actual_end = end_byte.min(current_chunk_data.len());

            let line = String::from_utf8_lossy(&current_chunk_data[start_byte..actual_end]);
            lines.push(line.trim_end_matches('\n').to_string());
        }

        Ok(lines)
    }

    /// Read all original lines. Use carefully for large files.
    pub fn read_all_original_lines(&self) -> Result<Vec<String>> {
        self.read_original_lines(0, self.index.total_lines)
    }

    /// Get the current state of all lines (after applying all operations).
    pub fn get_current_lines(&mut self) -> Result<Vec<String>> {
        if let Some(ref lines) = self.current_lines {
            return Ok(lines.clone());
        }

        let mut lines = self.read_all_original_lines()?;

        // Apply all operations in order
        for record in &self.operations {
            lines = record.operation.apply(lines)?;
        }

        self.current_lines = Some(lines.clone());
        Ok(lines)
    }

    /// Get the number of lines in current state.
    pub fn current_line_count(&mut self) -> Result<usize> {
        let lines = self.get_current_lines()?;
        Ok(lines.len())
    }

    /// Read lines from the current state (after operations).
    pub fn read_current_lines(&mut self, start: usize, count: usize) -> Result<Vec<String>> {
        let lines = self.get_current_lines()?;
        if start >= lines.len() {
            return Err(LogAnalyzerError::LineOutOfRange(start, lines.len()));
        }
        let end = (start + count).min(lines.len());
        Ok(lines[start..end].to_vec())
    }

    /// Apply an operation to the current state.
    pub fn apply_operation(&mut self, operation: Operation) -> Result<()> {
        // Get current lines and apply
        let lines = self.get_current_lines()?;
        let (new_lines, inverse) = operation.apply_with_inverse(lines)?;

        // Record the operation
        let record = OperationRecord {
            id: self.operations.len(),
            operation,
            inverse,
            applied_at: chrono::Utc::now(),
        };

        self.operations.push(record);
        self.current_lines = Some(new_lines);

        self.save_operations()?;
        Ok(())
    }

    /// Undo the last operation.
    pub fn undo(&mut self) -> Result<Operation> {
        let record = self
            .operations
            .pop()
            .ok_or(LogAnalyzerError::NoOperationsToUndo)?;

        // Invalidate cache and recompute
        self.current_lines = None;

        self.save_operations()?;
        Ok(record.operation)
    }

    /// Get operation history.
    pub fn history(&self) -> &[OperationRecord] {
        &self.operations
    }

    /// Export current state to a file.
    pub fn export(&mut self, dest: &Path) -> Result<()> {
        let lines = self.get_current_lines()?;
        let content = lines.join("\n");
        fs::write(dest, content)?;
        Ok(())
    }

    /// Get repository path.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Get total original line count.
    pub fn original_line_count(&self) -> usize {
        self.index.total_lines
    }

    /// Create a streaming line reader for chunk-by-chunk processing.
    /// Memory usage is O(chunk_size) instead of O(total_lines).
    pub fn line_stream(&self) -> LineStream<'_> {
        LineStream::new(&self.storage, &self.index)
    }

    /// Create a chunked processor for streaming operations on original data.
    /// Use this for large files where loading all lines is impractical.
    pub fn processor(&self) -> ChunkedProcessor<'_> {
        ChunkedProcessor::new(&self.storage, &self.index)
    }

    /// Get a reference to the storage.
    pub fn storage(&self) -> &ChunkStorage {
        &self.storage
    }

    fn save_operations(&self) -> Result<()> {
        let json = serde_json::to_string_pretty(&self.operations)?;
        fs::write(self.path.join("operations.json"), json)?;
        Ok(())
    }
}

fn copy_dir_all(src: &Path, dst: &Path) -> Result<()> {
    fs::create_dir_all(dst)?;
    for entry in fs::read_dir(src)? {
        let entry = entry?;
        let ty = entry.file_type()?;
        if ty.is_dir() {
            copy_dir_all(&entry.path(), &dst.join(entry.file_name()))?;
        } else {
            fs::copy(entry.path(), dst.join(entry.file_name()))?;
        }
    }
    Ok(())
}
