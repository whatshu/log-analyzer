use rayon::prelude::*;
use regex::Regex;

use super::InverseData;
use crate::error::Result;

const PARALLEL_THRESHOLD: usize = 10_000;

pub struct Filter;

impl Filter {
    pub fn apply(lines: Vec<String>, pattern: &str, keep: bool) -> Result<Vec<String>> {
        let re = Regex::new(pattern)?;

        let result = if lines.len() > PARALLEL_THRESHOLD {
            lines
                .into_par_iter()
                .filter(|line| re.is_match(line) == keep)
                .collect()
        } else {
            lines
                .into_iter()
                .filter(|line| re.is_match(line) == keep)
                .collect()
        };

        Ok(result)
    }

    pub fn apply_with_inverse(
        lines: Vec<String>,
        pattern: &str,
        keep: bool,
    ) -> Result<(Vec<String>, InverseData)> {
        let re = Regex::new(pattern)?;

        let mut kept = Vec::new();
        let mut removed = Vec::new();

        for (idx, line) in lines.into_iter().enumerate() {
            if re.is_match(&line) == keep {
                kept.push(line);
            } else {
                removed.push((idx, line));
            }
        }

        Ok((kept, InverseData::FilterInverse { removed }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_filter_keep() {
        let lines = vec![
            "ERROR: something failed".to_string(),
            "INFO: all good".to_string(),
            "ERROR: another failure".to_string(),
            "DEBUG: trace".to_string(),
        ];

        let result = Filter::apply(lines, "ERROR", true).unwrap();
        assert_eq!(result.len(), 2);
        assert!(result[0].contains("ERROR"));
        assert!(result[1].contains("ERROR"));
    }

    #[test]
    fn test_filter_remove() {
        let lines = vec![
            "ERROR: something failed".to_string(),
            "INFO: all good".to_string(),
            "DEBUG: trace".to_string(),
        ];

        let result = Filter::apply(lines, "DEBUG", false).unwrap();
        assert_eq!(result.len(), 2);
        assert!(!result.iter().any(|l| l.contains("DEBUG")));
    }

    #[test]
    fn test_filter_with_inverse() {
        let lines = vec![
            "line1".to_string(),
            "match_line2".to_string(),
            "line3".to_string(),
            "match_line4".to_string(),
        ];

        let (kept, inverse) =
            Filter::apply_with_inverse(lines, "match", true).unwrap();
        assert_eq!(kept.len(), 2);

        if let InverseData::FilterInverse { removed } = inverse {
            assert_eq!(removed.len(), 2);
            assert_eq!(removed[0], (0, "line1".to_string()));
            assert_eq!(removed[1], (2, "line3".to_string()));
        } else {
            panic!("Expected FilterInverse");
        }
    }
}
