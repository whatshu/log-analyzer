use super::InverseData;
use crate::error::{LogAnalyzerError, Result};

pub struct DeleteLines;
pub struct InsertLines;
pub struct ModifyLine;

impl DeleteLines {
    pub fn apply(mut lines: Vec<String>, indices: &[usize]) -> Result<Vec<String>> {
        // Validate indices
        for &idx in indices {
            if idx >= lines.len() {
                return Err(LogAnalyzerError::LineOutOfRange(idx, lines.len()));
            }
        }

        // Sort indices in descending order and remove
        let mut sorted_indices: Vec<usize> = indices.to_vec();
        sorted_indices.sort_unstable();
        sorted_indices.dedup();

        // Remove from back to front to preserve indices
        for &idx in sorted_indices.iter().rev() {
            lines.remove(idx);
        }

        Ok(lines)
    }

    pub fn apply_with_inverse(
        lines: Vec<String>,
        indices: &[usize],
    ) -> Result<(Vec<String>, InverseData)> {
        // Validate indices
        for &idx in indices {
            if idx >= lines.len() {
                return Err(LogAnalyzerError::LineOutOfRange(idx, lines.len()));
            }
        }

        let mut sorted_indices: Vec<usize> = indices.to_vec();
        sorted_indices.sort_unstable();
        sorted_indices.dedup();

        // Record deleted lines for undo
        let deleted: Vec<(usize, String)> = sorted_indices
            .iter()
            .map(|&idx| (idx, lines[idx].clone()))
            .collect();

        // Remove from back to front
        let mut result = lines;
        for &idx in sorted_indices.iter().rev() {
            result.remove(idx);
        }

        Ok((result, InverseData::DeleteInverse { deleted }))
    }
}

impl InsertLines {
    pub fn apply(
        mut lines: Vec<String>,
        after_line: usize,
        content: &[String],
    ) -> Result<Vec<String>> {
        if after_line > lines.len() {
            return Err(LogAnalyzerError::LineOutOfRange(after_line, lines.len()));
        }

        // Insert after the specified line (0 means insert at beginning)
        let insert_pos = after_line;
        for (i, line) in content.iter().enumerate() {
            lines.insert(insert_pos + i, line.clone());
        }

        Ok(lines)
    }

    pub fn apply_with_inverse(
        lines: Vec<String>,
        after_line: usize,
        content: &[String],
    ) -> Result<(Vec<String>, InverseData)> {
        let count = content.len();
        let result = Self::apply(lines, after_line, content)?;

        Ok((
            result,
            InverseData::InsertInverse {
                after_line,
                count,
            },
        ))
    }
}

impl ModifyLine {
    pub fn apply(
        mut lines: Vec<String>,
        line_index: usize,
        new_content: &str,
    ) -> Result<Vec<String>> {
        if line_index >= lines.len() {
            return Err(LogAnalyzerError::LineOutOfRange(line_index, lines.len()));
        }

        lines[line_index] = new_content.to_string();
        Ok(lines)
    }

    pub fn apply_with_inverse(
        lines: Vec<String>,
        line_index: usize,
        new_content: &str,
    ) -> Result<(Vec<String>, InverseData)> {
        if line_index >= lines.len() {
            return Err(LogAnalyzerError::LineOutOfRange(line_index, lines.len()));
        }

        let original_content = lines[line_index].clone();
        let mut result = lines;
        result[line_index] = new_content.to_string();

        Ok((
            result,
            InverseData::ModifyInverse {
                line_index,
                original_content,
            },
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_lines() -> Vec<String> {
        vec![
            "line0".to_string(),
            "line1".to_string(),
            "line2".to_string(),
            "line3".to_string(),
            "line4".to_string(),
        ]
    }

    #[test]
    fn test_delete_lines() {
        let lines = sample_lines();
        let result = DeleteLines::apply(lines, &[1, 3]).unwrap();
        assert_eq!(result, vec!["line0", "line2", "line4"]);
    }

    #[test]
    fn test_delete_lines_out_of_range() {
        let lines = sample_lines();
        assert!(DeleteLines::apply(lines, &[10]).is_err());
    }

    #[test]
    fn test_delete_with_inverse() {
        let lines = sample_lines();
        let (result, inverse) = DeleteLines::apply_with_inverse(lines, &[1, 3]).unwrap();
        assert_eq!(result, vec!["line0", "line2", "line4"]);

        if let InverseData::DeleteInverse { deleted } = inverse {
            assert_eq!(deleted, vec![(1, "line1".to_string()), (3, "line3".to_string())]);
        } else {
            panic!("Expected DeleteInverse");
        }
    }

    #[test]
    fn test_insert_lines() {
        let lines = sample_lines();
        let content = vec!["new_a".to_string(), "new_b".to_string()];
        let result = InsertLines::apply(lines, 2, &content).unwrap();
        assert_eq!(
            result,
            vec!["line0", "line1", "new_a", "new_b", "line2", "line3", "line4"]
        );
    }

    #[test]
    fn test_insert_at_beginning() {
        let lines = sample_lines();
        let content = vec!["header".to_string()];
        let result = InsertLines::apply(lines, 0, &content).unwrap();
        assert_eq!(result[0], "header");
        assert_eq!(result[1], "line0");
    }

    #[test]
    fn test_modify_line() {
        let lines = sample_lines();
        let result = ModifyLine::apply(lines, 2, "modified").unwrap();
        assert_eq!(result[2], "modified");
        assert_eq!(result.len(), 5);
    }

    #[test]
    fn test_modify_with_inverse() {
        let lines = sample_lines();
        let (result, inverse) =
            ModifyLine::apply_with_inverse(lines, 2, "modified").unwrap();
        assert_eq!(result[2], "modified");

        if let InverseData::ModifyInverse {
            line_index,
            original_content,
        } = inverse
        {
            assert_eq!(line_index, 2);
            assert_eq!(original_content, "line2");
        } else {
            panic!("Expected ModifyInverse");
        }
    }
}
