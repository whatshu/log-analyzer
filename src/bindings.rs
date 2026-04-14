use pyo3::prelude::*;
use std::path::PathBuf;

use crate::operator::Operation;
use crate::repo::LogRepo;

/// Python wrapper for LogRepo.
#[pyclass(name = "LogRepo")]
pub struct PyLogRepo {
    inner: LogRepo,
}

#[pymethods]
impl PyLogRepo {
    /// Import a text file into a new log repository.
    #[staticmethod]
    fn import_file(repo_path: &str, source_file: &str) -> PyResult<Self> {
        let repo = LogRepo::import(&PathBuf::from(repo_path), &PathBuf::from(source_file))?;
        Ok(Self { inner: repo })
    }

    /// Import raw text into a new log repository.
    #[staticmethod]
    fn import_text(repo_path: &str, text: &str, source_name: &str) -> PyResult<Self> {
        let repo =
            LogRepo::import_from_bytes(&PathBuf::from(repo_path), text.as_bytes(), source_name.to_string())?;
        Ok(Self { inner: repo })
    }

    /// Open an existing log repository.
    #[staticmethod]
    fn open(repo_path: &str) -> PyResult<Self> {
        let repo = LogRepo::open(&PathBuf::from(repo_path))?;
        Ok(Self { inner: repo })
    }

    /// Clone this repository to a new path.
    fn clone_to(&self, dest_path: &str) -> PyResult<PyLogRepo> {
        let new_repo = self.inner.clone_to(&PathBuf::from(dest_path))?;
        Ok(PyLogRepo { inner: new_repo })
    }

    /// Get repository metadata.
    fn metadata(&self) -> PyResult<PyRepoMetadata> {
        Ok(PyRepoMetadata {
            inner: self.inner.metadata.clone(),
        })
    }

    /// Get total original line count.
    fn original_line_count(&self) -> usize {
        self.inner.original_line_count()
    }

    /// Get current line count (after operations).
    fn current_line_count(&mut self) -> PyResult<usize> {
        Ok(self.inner.current_line_count()?)
    }

    /// Read lines from the current state.
    fn read_lines(&mut self, start: usize, count: usize) -> PyResult<Vec<String>> {
        Ok(self.inner.read_current_lines(start, count)?)
    }

    /// Read all current lines.
    fn read_all_lines(&mut self) -> PyResult<Vec<String>> {
        Ok(self.inner.get_current_lines()?)
    }

    /// Read a single line from the current state.
    fn read_line(&mut self, line_num: usize) -> PyResult<String> {
        let lines = self.inner.read_current_lines(line_num, 1)?;
        Ok(lines.into_iter().next().unwrap_or_default())
    }

    /// Apply a filter operation. `keep=True` keeps matching lines, `keep=False` removes them.
    fn filter(&mut self, pattern: &str, keep: bool) -> PyResult<()> {
        let op = Operation::Filter {
            pattern: pattern.to_string(),
            keep,
        };
        self.inner.apply_operation(op)?;
        Ok(())
    }

    /// Apply a regex replace operation.
    fn replace(&mut self, pattern: &str, replacement: &str) -> PyResult<()> {
        let op = Operation::Replace {
            pattern: pattern.to_string(),
            replacement: replacement.to_string(),
        };
        self.inner.apply_operation(op)?;
        Ok(())
    }

    /// Delete specific lines by their indices (0-based).
    fn delete_lines(&mut self, indices: Vec<usize>) -> PyResult<()> {
        let op = Operation::DeleteLines {
            line_indices: indices,
        };
        self.inner.apply_operation(op)?;
        Ok(())
    }

    /// Insert lines after the specified position (0 = insert at beginning).
    fn insert_lines(&mut self, after_line: usize, content: Vec<String>) -> PyResult<()> {
        let op = Operation::InsertLines {
            after_line,
            content,
        };
        self.inner.apply_operation(op)?;
        Ok(())
    }

    /// Modify a single line by index.
    fn modify_line(&mut self, line_index: usize, new_content: &str) -> PyResult<()> {
        let op = Operation::ModifyLine {
            line_index,
            new_content: new_content.to_string(),
        };
        self.inner.apply_operation(op)?;
        Ok(())
    }

    /// Undo the last operation. Returns a description of the undone operation.
    fn undo(&mut self) -> PyResult<String> {
        let op = self.inner.undo()?;
        Ok(op.describe())
    }

    /// Get operation history as a list of OperationRecord.
    fn history(&self) -> Vec<PyOperationRecord> {
        self.inner
            .history()
            .iter()
            .map(|r| PyOperationRecord {
                id: r.id,
                description: r.operation.describe(),
                applied_at: r.applied_at.to_rfc3339(),
            })
            .collect()
    }

    /// Export current state to a file.
    fn export(&mut self, dest_path: &str) -> PyResult<()> {
        self.inner.export(&PathBuf::from(dest_path))?;
        Ok(())
    }

    /// Get the repository path.
    fn path(&self) -> String {
        self.inner.path().to_string_lossy().to_string()
    }

    // --- Streaming engine methods (memory-efficient for large files) ---

    /// Count lines matching a regex in the original data, streaming chunk-by-chunk.
    /// Memory usage: O(chunk_size), not O(total_lines). Safe for >10GB files.
    fn count_matches(&self, pattern: &str) -> PyResult<usize> {
        Ok(self.inner.processor().count_matches(pattern)?)
    }

    /// Stream-filter original data to a file without loading all lines into memory.
    /// Returns the number of matching lines written.
    fn stream_filter_to_file(&self, pattern: &str, keep: bool, output: &str) -> PyResult<usize> {
        Ok(self
            .inner
            .processor()
            .filter_to_file(pattern, keep, &PathBuf::from(output), None)?)
    }

    /// Stream-replace in original data and write to a file.
    /// Returns the number of lines modified.
    fn stream_replace_to_file(
        &self,
        pattern: &str,
        replacement: &str,
        output: &str,
    ) -> PyResult<usize> {
        Ok(self
            .inner
            .processor()
            .replace_to_file(pattern, replacement, &PathBuf::from(output), None)?)
    }

    /// Search original data for lines matching a pattern, streaming chunk-by-chunk.
    /// Returns list of (line_number, line_content) tuples.
    fn stream_search(&self, pattern: &str, max_results: usize) -> PyResult<Vec<(usize, String)>> {
        Ok(self.inner.processor().search(pattern, max_results)?)
    }

    /// Parallel search across all chunks. Faster but results are collected then sorted.
    /// Returns list of (line_number, line_content) tuples.
    fn parallel_search(
        &self,
        pattern: &str,
        max_results: usize,
    ) -> PyResult<Vec<(usize, String)>> {
        Ok(self.inner.processor().parallel_search(pattern, max_results)?)
    }

    /// Export original data to a file, streaming chunk-by-chunk.
    /// Returns the number of lines written.
    fn stream_export(&self, output: &str) -> PyResult<usize> {
        Ok(self
            .inner
            .processor()
            .export_to_file(&PathBuf::from(output), None)?)
    }

    /// Compute statistics over the log without loading all data into memory.
    fn stats(&self) -> PyResult<PyLogStats> {
        let s = self.inner.processor().stats()?;
        Ok(PyLogStats {
            total_lines: s.total_lines,
            total_bytes: s.total_bytes,
            avg_line_len: s.avg_line_len,
            max_line_len: s.max_line_len,
            min_line_len: s.min_line_len,
            chunk_count: s.chunk_count,
        })
    }
}

/// Python wrapper for repository metadata.
#[pyclass(name = "RepoMetadata", from_py_object)]
#[derive(Clone)]
pub struct PyRepoMetadata {
    inner: crate::repo::RepoMetadata,
}

#[pymethods]
impl PyRepoMetadata {
    #[getter]
    fn id(&self) -> &str {
        &self.inner.id
    }

    #[getter]
    fn source_name(&self) -> &str {
        &self.inner.source_name
    }

    #[getter]
    fn original_size(&self) -> u64 {
        self.inner.original_size
    }

    #[getter]
    fn original_line_count(&self) -> usize {
        self.inner.original_line_count
    }

    #[getter]
    fn created_at(&self) -> String {
        self.inner.created_at.to_rfc3339()
    }

    #[getter]
    fn description(&self) -> Option<String> {
        self.inner.description.clone()
    }

    fn __repr__(&self) -> String {
        format!(
            "RepoMetadata(source='{}', lines={}, size={})",
            self.inner.source_name, self.inner.original_line_count, self.inner.original_size
        )
    }
}

/// Python wrapper for an operation history record.
#[pyclass(name = "OperationRecord", from_py_object)]
#[derive(Clone)]
pub struct PyOperationRecord {
    #[pyo3(get)]
    id: usize,
    #[pyo3(get)]
    description: String,
    #[pyo3(get)]
    applied_at: String,
}

#[pymethods]
impl PyOperationRecord {
    fn __repr__(&self) -> String {
        format!(
            "OperationRecord(id={}, desc='{}', at='{}')",
            self.id, self.description, self.applied_at
        )
    }
}

/// Log statistics computed in streaming fashion.
#[pyclass(name = "LogStats")]
pub struct PyLogStats {
    #[pyo3(get)]
    total_lines: usize,
    #[pyo3(get)]
    total_bytes: usize,
    #[pyo3(get)]
    avg_line_len: f64,
    #[pyo3(get)]
    max_line_len: usize,
    #[pyo3(get)]
    min_line_len: usize,
    #[pyo3(get)]
    chunk_count: usize,
}

#[pymethods]
impl PyLogStats {
    fn __repr__(&self) -> String {
        format!(
            "LogStats(lines={}, bytes={}, avg_len={:.1}, chunks={})",
            self.total_lines, self.total_bytes, self.avg_line_len, self.chunk_count
        )
    }
}
