mod crud;
mod filter;
mod replace;

pub use crud::{DeleteLines, InsertLines, ModifyLine};
pub use filter::Filter;
pub use replace::Replace;

use chrono::{DateTime, Utc};
use rayon::prelude::*;
use serde::{Deserialize, Serialize};

use crate::error::Result;

/// Represents a reversible operation on log lines.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Operation {
    /// Filter lines by regex pattern. `keep=true` keeps matches, `keep=false` removes them.
    Filter {
        pattern: String,
        keep: bool,
    },
    /// Replace matching text with a replacement string.
    Replace {
        pattern: String,
        replacement: String,
    },
    /// Delete specific lines by index.
    DeleteLines {
        line_indices: Vec<usize>,
    },
    /// Insert lines after a given position.
    InsertLines {
        after_line: usize,
        content: Vec<String>,
    },
    /// Modify a single line.
    ModifyLine {
        line_index: usize,
        new_content: String,
    },
}

/// Stored inverse data for undoing an operation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum InverseData {
    /// For filter: the removed lines with their original positions.
    FilterInverse {
        removed: Vec<(usize, String)>,
    },
    /// For replace: original lines that were modified.
    ReplaceInverse {
        originals: Vec<(usize, String)>,
    },
    /// For delete: the deleted lines with their original positions.
    DeleteInverse {
        deleted: Vec<(usize, String)>,
    },
    /// For insert: the count of inserted lines and position.
    InsertInverse {
        after_line: usize,
        count: usize,
    },
    /// For modify: the original content.
    ModifyInverse {
        line_index: usize,
        original_content: String,
    },
}

/// A recorded operation with its inverse for undo.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OperationRecord {
    pub id: usize,
    pub operation: Operation,
    pub inverse: InverseData,
    pub applied_at: DateTime<Utc>,
}

impl Operation {
    /// Apply this operation to lines and return the result.
    pub fn apply(&self, lines: Vec<String>) -> Result<Vec<String>> {
        match self {
            Operation::Filter { pattern, keep } => Filter::apply(lines, pattern, *keep),
            Operation::Replace {
                pattern,
                replacement,
            } => Replace::apply(lines, pattern, replacement),
            Operation::DeleteLines { line_indices } => DeleteLines::apply(lines, line_indices),
            Operation::InsertLines {
                after_line,
                content,
            } => InsertLines::apply(lines, *after_line, content),
            Operation::ModifyLine {
                line_index,
                new_content,
            } => ModifyLine::apply(lines, *line_index, new_content),
        }
    }

    /// Apply this operation and also compute the inverse data for undo.
    pub fn apply_with_inverse(&self, lines: Vec<String>) -> Result<(Vec<String>, InverseData)> {
        match self {
            Operation::Filter { pattern, keep } => {
                Filter::apply_with_inverse(lines, pattern, *keep)
            }
            Operation::Replace {
                pattern,
                replacement,
            } => Replace::apply_with_inverse(lines, pattern, replacement),
            Operation::DeleteLines { line_indices } => {
                DeleteLines::apply_with_inverse(lines, line_indices)
            }
            Operation::InsertLines {
                after_line,
                content,
            } => InsertLines::apply_with_inverse(lines, *after_line, content),
            Operation::ModifyLine {
                line_index,
                new_content,
            } => ModifyLine::apply_with_inverse(lines, *line_index, new_content),
        }
    }

    /// Get a human-readable description of this operation.
    pub fn describe(&self) -> String {
        match self {
            Operation::Filter { pattern, keep } => {
                if *keep {
                    format!("filter keep /{}/", pattern)
                } else {
                    format!("filter remove /{}/", pattern)
                }
            }
            Operation::Replace {
                pattern,
                replacement,
            } => {
                format!("replace /{}/ -> \"{}\"", pattern, replacement)
            }
            Operation::DeleteLines { line_indices } => {
                if line_indices.len() <= 5 {
                    format!("delete lines {:?}", line_indices)
                } else {
                    format!("delete {} lines", line_indices.len())
                }
            }
            Operation::InsertLines {
                after_line,
                content,
            } => {
                format!("insert {} lines after line {}", content.len(), after_line)
            }
            Operation::ModifyLine {
                line_index,
                new_content: _,
            } => {
                format!("modify line {}", line_index)
            }
        }
    }
}

/// Apply an operation in parallel across chunks of lines.
/// Used by filter and replace for large datasets.
pub fn parallel_apply<F>(lines: Vec<String>, chunk_size: usize, f: F) -> Vec<String>
where
    F: Fn(&str) -> Option<String> + Send + Sync,
{
    if lines.len() < chunk_size * 2 {
        // Small dataset: sequential
        lines
            .into_iter()
            .filter_map(|line| f(&line))
            .collect()
    } else {
        // Large dataset: parallel
        lines
            .into_par_iter()
            .filter_map(|line| f(&line))
            .collect()
    }
}
