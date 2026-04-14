use rayon::prelude::*;
use regex::Regex;

use super::InverseData;
use crate::error::Result;

const PARALLEL_THRESHOLD: usize = 10_000;

pub struct Replace;

impl Replace {
    pub fn apply(lines: Vec<String>, pattern: &str, replacement: &str) -> Result<Vec<String>> {
        let re = Regex::new(pattern)?;

        let result = if lines.len() > PARALLEL_THRESHOLD {
            lines
                .into_par_iter()
                .map(|line| re.replace_all(&line, replacement).into_owned())
                .collect()
        } else {
            lines
                .into_iter()
                .map(|line| re.replace_all(&line, replacement).into_owned())
                .collect()
        };

        Ok(result)
    }

    pub fn apply_with_inverse(
        lines: Vec<String>,
        pattern: &str,
        replacement: &str,
    ) -> Result<(Vec<String>, InverseData)> {
        let re = Regex::new(pattern)?;

        let mut originals = Vec::new();
        let mut new_lines = Vec::with_capacity(lines.len());

        for (idx, line) in lines.into_iter().enumerate() {
            let replaced = re.replace_all(&line, replacement).into_owned();
            if replaced != line {
                originals.push((idx, line));
            }
            new_lines.push(replaced);
        }

        Ok((new_lines, InverseData::ReplaceInverse { originals }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_replace_simple() {
        let lines = vec![
            "2024-01-01 ERROR: fail".to_string(),
            "2024-01-01 INFO: ok".to_string(),
        ];

        let result = Replace::apply(lines, r"\d{4}-\d{2}-\d{2}", "DATE").unwrap();
        assert_eq!(result[0], "DATE ERROR: fail");
        assert_eq!(result[1], "DATE INFO: ok");
    }

    #[test]
    fn test_replace_with_capture_groups() {
        let lines = vec!["user=alice action=login".to_string()];

        let result = Replace::apply(lines, r"user=(\w+)", "user=[$1]").unwrap();
        assert_eq!(result[0], "user=[alice] action=login");
    }

    #[test]
    fn test_replace_with_inverse() {
        let lines = vec![
            "hello world".to_string(),
            "foo bar".to_string(),
            "hello foo".to_string(),
        ];

        let (new_lines, inverse) =
            Replace::apply_with_inverse(lines, "hello", "hi").unwrap();

        assert_eq!(new_lines[0], "hi world");
        assert_eq!(new_lines[1], "foo bar");
        assert_eq!(new_lines[2], "hi foo");

        if let InverseData::ReplaceInverse { originals } = inverse {
            assert_eq!(originals.len(), 2);
            assert_eq!(originals[0], (0, "hello world".to_string()));
            assert_eq!(originals[1], (2, "hello foo".to_string()));
        } else {
            panic!("Expected ReplaceInverse");
        }
    }
}
