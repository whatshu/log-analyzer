//! Collector — a read-only terminal operation on a line stream.
//!
//! Design inspired by:
//! - **Java Stream `Collector`**: supplier → accumulator → combiner → finisher
//! - **Rust `Iterator::fold`** / `reduce` / `collect`
//!
//! A `Collector` defines *what* to compute. The engine feeds lines into it
//! chunk-by-chunk via the `Accumulator` trait, and chunks can be processed
//! in parallel then merged. The result is returned without modifying the
//! repository.
//!
//! ```text
//!   repo.stream()          // LineStream (chunk iterator)
//!       .collect(collector) // terminal — produces CollectResult
//! ```

use std::collections::HashMap;

use rayon::prelude::*;
use regex::Regex;
use serde::{Deserialize, Serialize};

use crate::error::Result;
use crate::index::LineIndex;
use crate::repo::ChunkStorage;

use super::read_chunk_lines;

// ---------------------------------------------------------------------------
// Public types
// ---------------------------------------------------------------------------

/// Describes what to collect from a line stream.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Collector {
    /// Count lines, optionally filtered by a regex.
    Count {
        pattern: Option<String>,
    },

    /// Group lines by a regex capture group and count each group.
    /// `group_index` is the 1-based capture group number.
    GroupCount {
        pattern: String,
        group_index: usize,
    },

    /// Top-N most frequent values of a regex capture group.
    TopN {
        pattern: String,
        group_index: usize,
        n: usize,
    },

    /// Collect distinct values of a regex capture group.
    Unique {
        pattern: String,
        group_index: usize,
    },

    /// Compute numeric statistics (min/max/avg/sum) from a regex capture.
    /// The captured text is parsed as `f64`.
    NumericStats {
        pattern: String,
        group_index: usize,
    },

    /// Compute line-length statistics over all lines.
    LineStats,
}

/// The result of a collect operation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum CollectResult {
    Count(usize),

    /// Sorted by count descending.
    GroupCount(Vec<(String, usize)>),

    /// Top-N entries sorted by count descending.
    TopN(Vec<(String, usize)>),

    /// Sorted lexicographically.
    Unique(Vec<String>),

    NumericStats {
        count: usize,
        sum: f64,
        min: f64,
        max: f64,
        avg: f64,
    },

    LineStats {
        count: usize,
        total_bytes: usize,
        avg_len: f64,
        max_len: usize,
        min_len: usize,
    },
}

// ---------------------------------------------------------------------------
// Accumulator — the fold state for each collector kind
// ---------------------------------------------------------------------------

enum Acc {
    Count(usize),
    GroupCount(HashMap<String, usize>),
    Unique(HashMap<String, ()>),
    NumericStats {
        count: usize,
        sum: f64,
        min: f64,
        max: f64,
    },
    LineStats {
        count: usize,
        total_bytes: usize,
        max_len: usize,
        min_len: usize,
    },
}

impl Acc {
    /// Create a fresh accumulator matching the collector kind.
    fn new(collector: &Collector) -> Self {
        match collector {
            Collector::Count { .. } => Acc::Count(0),
            Collector::GroupCount { .. } | Collector::TopN { .. } => {
                Acc::GroupCount(HashMap::new())
            }
            Collector::Unique { .. } => Acc::Unique(HashMap::new()),
            Collector::NumericStats { .. } => Acc::NumericStats {
                count: 0,
                sum: 0.0,
                min: f64::INFINITY,
                max: f64::NEG_INFINITY,
            },
            Collector::LineStats => Acc::LineStats {
                count: 0,
                total_bytes: 0,
                max_len: 0,
                min_len: usize::MAX,
            },
        }
    }

    /// Fold a single line into the accumulator.
    fn accumulate(&mut self, line: &str, re: Option<&Regex>, group_index: usize) {
        match self {
            Acc::Count(n) => {
                if let Some(re) = re {
                    if re.is_match(line) {
                        *n += 1;
                    }
                } else {
                    *n += 1;
                }
            }
            Acc::GroupCount(map) => {
                if let Some(re) = re {
                    if let Some(caps) = re.captures(line) {
                        if let Some(m) = caps.get(group_index) {
                            *map.entry(m.as_str().to_string()).or_insert(0) += 1;
                        }
                    }
                }
            }
            Acc::Unique(set) => {
                if let Some(re) = re {
                    if let Some(caps) = re.captures(line) {
                        if let Some(m) = caps.get(group_index) {
                            set.entry(m.as_str().to_string()).or_insert(());
                        }
                    }
                }
            }
            Acc::NumericStats {
                count,
                sum,
                min,
                max,
            } => {
                if let Some(re) = re {
                    if let Some(caps) = re.captures(line) {
                        if let Some(m) = caps.get(group_index) {
                            if let Ok(val) = m.as_str().parse::<f64>() {
                                *count += 1;
                                *sum += val;
                                if val < *min {
                                    *min = val;
                                }
                                if val > *max {
                                    *max = val;
                                }
                            }
                        }
                    }
                }
            }
            Acc::LineStats {
                count,
                total_bytes,
                max_len,
                min_len,
            } => {
                let len = line.len();
                *count += 1;
                *total_bytes += len;
                if len > *max_len {
                    *max_len = len;
                }
                if len < *min_len {
                    *min_len = len;
                }
            }
        }
    }

    /// Merge another accumulator into self (combiner for parallel).
    fn merge(&mut self, other: Acc) {
        match (self, other) {
            (Acc::Count(a), Acc::Count(b)) => *a += b,
            (Acc::GroupCount(a), Acc::GroupCount(b)) => {
                for (k, v) in b {
                    *a.entry(k).or_insert(0) += v;
                }
            }
            (Acc::Unique(a), Acc::Unique(b)) => {
                for (k, _) in b {
                    a.entry(k).or_insert(());
                }
            }
            (
                Acc::NumericStats {
                    count: c1,
                    sum: s1,
                    min: mn1,
                    max: mx1,
                },
                Acc::NumericStats {
                    count: c2,
                    sum: s2,
                    min: mn2,
                    max: mx2,
                },
            ) => {
                *c1 += c2;
                *s1 += s2;
                if mn2 < *mn1 {
                    *mn1 = mn2;
                }
                if mx2 > *mx1 {
                    *mx1 = mx2;
                }
            }
            (
                Acc::LineStats {
                    count: c1,
                    total_bytes: t1,
                    max_len: mx1,
                    min_len: mn1,
                },
                Acc::LineStats {
                    count: c2,
                    total_bytes: t2,
                    max_len: mx2,
                    min_len: mn2,
                },
            ) => {
                *c1 += c2;
                *t1 += t2;
                if mx2 > *mx1 {
                    *mx1 = mx2;
                }
                if mn2 < *mn1 {
                    *mn1 = mn2;
                }
            }
            _ => {} // mismatched types — should never happen
        }
    }

    /// Finisher: convert accumulator into a CollectResult.
    fn finish(self, collector: &Collector) -> CollectResult {
        match (self, collector) {
            (Acc::Count(n), _) => CollectResult::Count(n),

            (Acc::GroupCount(map), Collector::TopN { n, .. }) => {
                let mut pairs: Vec<(String, usize)> = map.into_iter().collect();
                pairs.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
                pairs.truncate(*n);
                CollectResult::TopN(pairs)
            }
            (Acc::GroupCount(map), _) => {
                let mut pairs: Vec<(String, usize)> = map.into_iter().collect();
                pairs.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
                CollectResult::GroupCount(pairs)
            }

            (Acc::Unique(set), _) => {
                let mut vals: Vec<String> = set.into_keys().collect();
                vals.sort();
                CollectResult::Unique(vals)
            }

            (Acc::NumericStats { count, sum, min, max }, _) => {
                let avg = if count > 0 { sum / count as f64 } else { 0.0 };
                let min = if count > 0 { min } else { 0.0 };
                let max = if count > 0 { max } else { 0.0 };
                CollectResult::NumericStats { count, sum, min, max, avg }
            }

            (Acc::LineStats { count, total_bytes, max_len, min_len }, _) => {
                let avg_len = if count > 0 {
                    total_bytes as f64 / count as f64
                } else {
                    0.0
                };
                let min_len = if count > 0 { min_len } else { 0 };
                CollectResult::LineStats { count, total_bytes, avg_len, max_len, min_len }
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Execution: run a collector over chunked storage
// ---------------------------------------------------------------------------

/// Execute a collector over the original log data, chunk-by-chunk in parallel.
/// Does **not** modify the repository.
pub fn execute(
    collector: &Collector,
    storage: &ChunkStorage,
    index: &LineIndex,
) -> Result<CollectResult> {
    // Compile regex once
    let re = match collector {
        Collector::Count { pattern: Some(p) } => Some(Regex::new(p)?),
        Collector::GroupCount { pattern, .. }
        | Collector::TopN { pattern, .. }
        | Collector::Unique { pattern, .. }
        | Collector::NumericStats { pattern, .. } => Some(Regex::new(pattern)?),
        _ => None,
    };

    let group_index = match collector {
        Collector::GroupCount { group_index, .. }
        | Collector::TopN { group_index, .. }
        | Collector::Unique { group_index, .. }
        | Collector::NumericStats { group_index, .. } => *group_index,
        _ => 0,
    };

    let total_chunks = index.chunks.len();

    // Parallel map: each chunk → its own Acc
    let accs: Vec<Result<Acc>> = (0..total_chunks)
        .into_par_iter()
        .map(|chunk_idx| {
            let lines = read_chunk_lines(storage, index, chunk_idx)?;
            let mut acc = Acc::new(collector);
            for line in &lines {
                acc.accumulate(line, re.as_ref(), group_index);
            }
            Ok(acc)
        })
        .collect();

    // Sequential reduce (merge)
    let mut combined = Acc::new(collector);
    for acc in accs {
        combined.merge(acc?);
    }

    Ok(combined.finish(collector))
}

// ---------------------------------------------------------------------------
// Execute a collector over an already-materialized Vec<String>
// (used when operations have been applied and lines are in memory)
// ---------------------------------------------------------------------------

/// Execute a collector over an in-memory slice of lines.
pub fn execute_on_lines(
    collector: &Collector,
    lines: &[String],
) -> Result<CollectResult> {
    let re = match collector {
        Collector::Count { pattern: Some(p) } => Some(Regex::new(p)?),
        Collector::GroupCount { pattern, .. }
        | Collector::TopN { pattern, .. }
        | Collector::Unique { pattern, .. }
        | Collector::NumericStats { pattern, .. } => Some(Regex::new(pattern)?),
        _ => None,
    };

    let group_index = match collector {
        Collector::GroupCount { group_index, .. }
        | Collector::TopN { group_index, .. }
        | Collector::Unique { group_index, .. }
        | Collector::NumericStats { group_index, .. } => *group_index,
        _ => 0,
    };

    let mut acc = Acc::new(collector);
    for line in lines {
        acc.accumulate(line, re.as_ref(), group_index);
    }
    Ok(acc.finish(collector))
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::index::IndexBuilder;
    use crate::repo::ChunkStorage;
    use tempfile::TempDir;

    fn setup(lines: &[&str]) -> (TempDir, ChunkStorage, LineIndex) {
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
    fn test_count_all() {
        let (_tmp, storage, index) = setup(&["a", "b", "c", "d"]);
        let r = execute(&Collector::Count { pattern: None }, &storage, &index).unwrap();
        assert!(matches!(r, CollectResult::Count(4)));
    }

    #[test]
    fn test_count_pattern() {
        let lines = vec!["INFO ok", "ERROR fail", "INFO ok", "ERROR boom"];
        let (_tmp, storage, index) = setup(&lines);
        let r = execute(
            &Collector::Count {
                pattern: Some("ERROR".into()),
            },
            &storage,
            &index,
        )
        .unwrap();
        assert!(matches!(r, CollectResult::Count(2)));
    }

    #[test]
    fn test_group_count() {
        let lines = vec![
            "[INFO] a",
            "[ERROR] b",
            "[INFO] c",
            "[ERROR] d",
            "[WARN] e",
        ];
        let (_tmp, storage, index) = setup(&lines);
        let r = execute(
            &Collector::GroupCount {
                pattern: r"\[(\w+)\]".into(),
                group_index: 1,
            },
            &storage,
            &index,
        )
        .unwrap();

        if let CollectResult::GroupCount(pairs) = r {
            let map: HashMap<_, _> = pairs.into_iter().collect();
            assert_eq!(map["INFO"], 2);
            assert_eq!(map["ERROR"], 2);
            assert_eq!(map["WARN"], 1);
        } else {
            panic!("expected GroupCount");
        }
    }

    #[test]
    fn test_top_n() {
        let lines = vec![
            "ip=1.1.1.1",
            "ip=2.2.2.2",
            "ip=1.1.1.1",
            "ip=3.3.3.3",
            "ip=1.1.1.1",
            "ip=2.2.2.2",
        ];
        let (_tmp, storage, index) = setup(&lines);
        let r = execute(
            &Collector::TopN {
                pattern: r"ip=(\S+)".into(),
                group_index: 1,
                n: 2,
            },
            &storage,
            &index,
        )
        .unwrap();

        if let CollectResult::TopN(pairs) = r {
            assert_eq!(pairs.len(), 2);
            assert_eq!(pairs[0], ("1.1.1.1".into(), 3));
            assert_eq!(pairs[1], ("2.2.2.2".into(), 2));
        } else {
            panic!("expected TopN");
        }
    }

    #[test]
    fn test_unique() {
        let lines = vec![
            "user=alice",
            "user=bob",
            "user=alice",
            "user=charlie",
            "user=bob",
        ];
        let (_tmp, storage, index) = setup(&lines);
        let r = execute(
            &Collector::Unique {
                pattern: r"user=(\w+)".into(),
                group_index: 1,
            },
            &storage,
            &index,
        )
        .unwrap();

        if let CollectResult::Unique(vals) = r {
            assert_eq!(vals, vec!["alice", "bob", "charlie"]);
        } else {
            panic!("expected Unique");
        }
    }

    #[test]
    fn test_numeric_stats() {
        let lines = vec![
            "latency=10ms",
            "latency=50ms",
            "latency=20ms",
            "latency=100ms",
        ];
        let (_tmp, storage, index) = setup(&lines);
        let r = execute(
            &Collector::NumericStats {
                pattern: r"latency=(\d+)ms".into(),
                group_index: 1,
            },
            &storage,
            &index,
        )
        .unwrap();

        if let CollectResult::NumericStats { count, sum, min, max, avg } = r {
            assert_eq!(count, 4);
            assert!((sum - 180.0).abs() < 0.001);
            assert!((min - 10.0).abs() < 0.001);
            assert!((max - 100.0).abs() < 0.001);
            assert!((avg - 45.0).abs() < 0.001);
        } else {
            panic!("expected NumericStats");
        }
    }

    #[test]
    fn test_line_stats() {
        let lines = vec!["short", "a medium length line here", "x"];
        let (_tmp, storage, index) = setup(&lines);
        let r = execute(&Collector::LineStats, &storage, &index).unwrap();

        if let CollectResult::LineStats { count, max_len, min_len, .. } = r {
            assert_eq!(count, 3);
            assert_eq!(max_len, 25);
            assert_eq!(min_len, 1);
        } else {
            panic!("expected LineStats");
        }
    }

    #[test]
    fn test_execute_on_lines() {
        let lines: Vec<String> = vec!["a".into(), "b".into(), "a".into(), "c".into()];
        let r = execute_on_lines(
            &Collector::GroupCount {
                pattern: r"(.)".into(),
                group_index: 1,
            },
            &lines,
        )
        .unwrap();

        if let CollectResult::GroupCount(pairs) = r {
            let map: HashMap<_, _> = pairs.into_iter().collect();
            assert_eq!(map["a"], 2);
            assert_eq!(map["b"], 1);
            assert_eq!(map["c"], 1);
        } else {
            panic!("expected GroupCount");
        }
    }

    #[test]
    fn test_empty_input() {
        // setup joins with "\n" + trailing "\n", so truly empty = use empty bytes directly
        let tmp = TempDir::new().unwrap();
        let chunks_dir = tmp.path().join("chunks");
        std::fs::create_dir_all(&chunks_dir).unwrap();

        let builder = IndexBuilder::new().with_lines_per_chunk(3);
        let (index, chunks_data) = builder.build(b"");

        let storage = ChunkStorage::new(chunks_dir);
        storage.write_chunks(&chunks_data).unwrap();

        let r = execute(&Collector::Count { pattern: None }, &storage, &index).unwrap();
        assert!(matches!(r, CollectResult::Count(0)));
    }

    #[test]
    fn test_numeric_stats_no_match() {
        let lines = vec!["no numbers here", "still nothing"];
        let (_tmp, storage, index) = setup(&lines);
        let r = execute(
            &Collector::NumericStats {
                pattern: r"val=(\d+)".into(),
                group_index: 1,
            },
            &storage,
            &index,
        )
        .unwrap();

        if let CollectResult::NumericStats { count, avg, .. } = r {
            assert_eq!(count, 0);
            assert!((avg - 0.0).abs() < 0.001);
        } else {
            panic!("expected NumericStats");
        }
    }
}
