use std::fs;
use tempfile::TempDir;

use log_analyzer_core::operator::Operation;
use log_analyzer_core::repo::LogRepo;

fn create_test_log(lines: usize) -> String {
    (0..lines)
        .map(|i| {
            let level = match i % 4 {
                0 => "INFO",
                1 => "WARN",
                2 => "ERROR",
                3 => "DEBUG",
                _ => unreachable!(),
            };
            format!(
                "2024-01-{:02} {:02}:{:02}:{:02} {} [thread-{}] message number {}",
                (i % 28) + 1,
                i % 24,
                i % 60,
                i % 60,
                level,
                i % 8,
                i
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
}

#[test]
fn test_import_and_read() {
    let tmp = TempDir::new().unwrap();
    let log_file = tmp.path().join("test.log");
    let repo_path = tmp.path().join("repo");

    let content = "line0\nline1\nline2\nline3\nline4\n";
    fs::write(&log_file, content).unwrap();

    let repo = LogRepo::import(&repo_path, &log_file).unwrap();
    assert_eq!(repo.original_line_count(), 5);

    let line = repo.read_original_line(0).unwrap();
    assert_eq!(line, "line0");

    let line = repo.read_original_line(4).unwrap();
    assert_eq!(line, "line4");
}

#[test]
fn test_import_from_bytes() {
    let tmp = TempDir::new().unwrap();
    let repo_path = tmp.path().join("repo");

    let data = b"hello\nworld\n";
    let repo = LogRepo::import_from_bytes(&repo_path, data, "test".into()).unwrap();

    assert_eq!(repo.original_line_count(), 2);
    assert_eq!(repo.read_original_line(0).unwrap(), "hello");
    assert_eq!(repo.read_original_line(1).unwrap(), "world");
}

#[test]
fn test_open_existing_repo() {
    let tmp = TempDir::new().unwrap();
    let repo_path = tmp.path().join("repo");

    let data = b"line0\nline1\nline2\n";
    LogRepo::import_from_bytes(&repo_path, data, "test".into()).unwrap();

    // Re-open
    let repo = LogRepo::open(&repo_path).unwrap();
    assert_eq!(repo.original_line_count(), 3);
    assert_eq!(repo.read_original_line(1).unwrap(), "line1");
}

#[test]
fn test_clone_repo() {
    let tmp = TempDir::new().unwrap();
    let repo_path = tmp.path().join("repo");
    let clone_path = tmp.path().join("repo_clone");

    let data = b"a\nb\nc\n";
    let repo = LogRepo::import_from_bytes(&repo_path, data, "test".into()).unwrap();

    let cloned = repo.clone_to(&clone_path).unwrap();
    assert_eq!(cloned.original_line_count(), 3);
    assert_eq!(cloned.read_original_line(0).unwrap(), "a");
}

#[test]
fn test_filter_operation() {
    let tmp = TempDir::new().unwrap();
    let repo_path = tmp.path().join("repo");

    let log_data = create_test_log(100);
    let mut repo =
        LogRepo::import_from_bytes(&repo_path, log_data.as_bytes(), "test.log".into()).unwrap();
    assert_eq!(repo.original_line_count(), 100);

    // Filter to keep only ERROR lines
    repo.apply_operation(Operation::Filter {
        pattern: "ERROR".to_string(),
        keep: true,
    })
    .unwrap();

    let count = repo.current_line_count().unwrap();
    assert_eq!(count, 25); // Every 4th line is ERROR

    let lines = repo.get_current_lines().unwrap();
    for line in &lines {
        assert!(line.contains("ERROR"));
    }
}

#[test]
fn test_replace_operation() {
    let tmp = TempDir::new().unwrap();
    let repo_path = tmp.path().join("repo");

    let data = b"hello world\nfoo bar\nhello foo\n";
    let mut repo = LogRepo::import_from_bytes(&repo_path, data, "test".into()).unwrap();

    repo.apply_operation(Operation::Replace {
        pattern: "hello".to_string(),
        replacement: "HI".to_string(),
    })
    .unwrap();

    let lines = repo.get_current_lines().unwrap();
    assert_eq!(lines[0], "HI world");
    assert_eq!(lines[1], "foo bar");
    assert_eq!(lines[2], "HI foo");
}

#[test]
fn test_delete_lines_operation() {
    let tmp = TempDir::new().unwrap();
    let repo_path = tmp.path().join("repo");

    let data = b"a\nb\nc\nd\ne\n";
    let mut repo = LogRepo::import_from_bytes(&repo_path, data, "test".into()).unwrap();

    repo.apply_operation(Operation::DeleteLines {
        line_indices: vec![1, 3],
    })
    .unwrap();

    let lines = repo.get_current_lines().unwrap();
    assert_eq!(lines, vec!["a", "c", "e"]);
}

#[test]
fn test_insert_lines_operation() {
    let tmp = TempDir::new().unwrap();
    let repo_path = tmp.path().join("repo");

    let data = b"a\nb\nc\n";
    let mut repo = LogRepo::import_from_bytes(&repo_path, data, "test".into()).unwrap();

    repo.apply_operation(Operation::InsertLines {
        after_line: 1,
        content: vec!["x".to_string(), "y".to_string()],
    })
    .unwrap();

    let lines = repo.get_current_lines().unwrap();
    assert_eq!(lines, vec!["a", "x", "y", "b", "c"]);
}

#[test]
fn test_modify_line_operation() {
    let tmp = TempDir::new().unwrap();
    let repo_path = tmp.path().join("repo");

    let data = b"a\nb\nc\n";
    let mut repo = LogRepo::import_from_bytes(&repo_path, data, "test".into()).unwrap();

    repo.apply_operation(Operation::ModifyLine {
        line_index: 1,
        new_content: "modified".to_string(),
    })
    .unwrap();

    let lines = repo.get_current_lines().unwrap();
    assert_eq!(lines, vec!["a", "modified", "c"]);
}

#[test]
fn test_undo_filter() {
    let tmp = TempDir::new().unwrap();
    let repo_path = tmp.path().join("repo");

    let data = b"keep_a\nremove_b\nkeep_c\nremove_d\n";
    let mut repo = LogRepo::import_from_bytes(&repo_path, data, "test".into()).unwrap();

    // Apply filter
    repo.apply_operation(Operation::Filter {
        pattern: "keep".to_string(),
        keep: true,
    })
    .unwrap();
    assert_eq!(repo.current_line_count().unwrap(), 2);

    // Undo
    repo.undo().unwrap();
    assert_eq!(repo.current_line_count().unwrap(), 4);

    let lines = repo.get_current_lines().unwrap();
    assert_eq!(lines, vec!["keep_a", "remove_b", "keep_c", "remove_d"]);
}

#[test]
fn test_undo_replace() {
    let tmp = TempDir::new().unwrap();
    let repo_path = tmp.path().join("repo");

    let data = b"hello world\nfoo bar\n";
    let mut repo = LogRepo::import_from_bytes(&repo_path, data, "test".into()).unwrap();

    repo.apply_operation(Operation::Replace {
        pattern: "hello".to_string(),
        replacement: "HI".to_string(),
    })
    .unwrap();

    let lines = repo.get_current_lines().unwrap();
    assert_eq!(lines[0], "HI world");

    repo.undo().unwrap();
    let lines = repo.get_current_lines().unwrap();
    assert_eq!(lines[0], "hello world");
}

#[test]
fn test_undo_delete() {
    let tmp = TempDir::new().unwrap();
    let repo_path = tmp.path().join("repo");

    let data = b"a\nb\nc\n";
    let mut repo = LogRepo::import_from_bytes(&repo_path, data, "test".into()).unwrap();

    repo.apply_operation(Operation::DeleteLines {
        line_indices: vec![1],
    })
    .unwrap();
    assert_eq!(repo.current_line_count().unwrap(), 2);

    repo.undo().unwrap();
    assert_eq!(repo.current_line_count().unwrap(), 3);

    let lines = repo.get_current_lines().unwrap();
    assert_eq!(lines, vec!["a", "b", "c"]);
}

#[test]
fn test_multiple_operations_and_undo() {
    let tmp = TempDir::new().unwrap();
    let repo_path = tmp.path().join("repo");

    let log_data = create_test_log(50);
    let mut repo =
        LogRepo::import_from_bytes(&repo_path, log_data.as_bytes(), "test.log".into()).unwrap();

    let original_count = repo.original_line_count();

    // Op 1: filter to ERROR
    repo.apply_operation(Operation::Filter {
        pattern: "ERROR".to_string(),
        keep: true,
    })
    .unwrap();
    let after_filter = repo.current_line_count().unwrap();

    // Op 2: replace timestamps
    repo.apply_operation(Operation::Replace {
        pattern: r"\d{4}-\d{2}-\d{2}".to_string(),
        replacement: "DATE".to_string(),
    })
    .unwrap();

    // Check history
    assert_eq!(repo.history().len(), 2);

    // Undo replace
    repo.undo().unwrap();
    assert_eq!(repo.history().len(), 1);
    assert_eq!(repo.current_line_count().unwrap(), after_filter);

    // Undo filter
    repo.undo().unwrap();
    assert_eq!(repo.history().len(), 0);
    assert_eq!(repo.current_line_count().unwrap(), original_count);
}

#[test]
fn test_export() {
    let tmp = TempDir::new().unwrap();
    let repo_path = tmp.path().join("repo");
    let export_path = tmp.path().join("exported.log");

    let data = b"line1\nline2\nline3\n";
    let mut repo = LogRepo::import_from_bytes(&repo_path, data, "test".into()).unwrap();

    // Apply a filter
    repo.apply_operation(Operation::Filter {
        pattern: "line[12]".to_string(),
        keep: true,
    })
    .unwrap();

    repo.export(&export_path).unwrap();

    let exported = fs::read_to_string(&export_path).unwrap();
    assert_eq!(exported, "line1\nline2");
}

#[test]
fn test_operations_persist_across_reopen() {
    let tmp = TempDir::new().unwrap();
    let repo_path = tmp.path().join("repo");

    let data = b"a\nb\nc\nd\ne\n";
    {
        let mut repo = LogRepo::import_from_bytes(&repo_path, data, "test".into()).unwrap();
        repo.apply_operation(Operation::Filter {
            pattern: "[ace]".to_string(),
            keep: true,
        })
        .unwrap();
    }

    // Reopen and verify operations are preserved
    let mut repo = LogRepo::open(&repo_path).unwrap();
    assert_eq!(repo.history().len(), 1);

    let lines = repo.get_current_lines().unwrap();
    assert_eq!(lines, vec!["a", "c", "e"]);
}

#[test]
fn test_metadata() {
    let tmp = TempDir::new().unwrap();
    let repo_path = tmp.path().join("repo");

    let data = b"hello\nworld\n";
    let repo = LogRepo::import_from_bytes(&repo_path, data, "myfile.log".into()).unwrap();

    assert_eq!(repo.metadata.source_name, "myfile.log");
    assert_eq!(repo.metadata.original_size, 12);
    assert_eq!(repo.metadata.original_line_count, 2);
    assert!(!repo.metadata.id.is_empty());
}

#[test]
fn test_large_log_chunking() {
    let tmp = TempDir::new().unwrap();
    let repo_path = tmp.path().join("repo");

    // Create a log with enough lines to span multiple chunks
    let log_data = create_test_log(25_000);
    let repo =
        LogRepo::import_from_bytes(&repo_path, log_data.as_bytes(), "large.log".into()).unwrap();

    assert_eq!(repo.original_line_count(), 25_000);

    // Verify random access works across chunks
    let line_0 = repo.read_original_line(0).unwrap();
    assert!(line_0.contains("message number 0"));

    let line_9999 = repo.read_original_line(9_999).unwrap();
    assert!(line_9999.contains("message number 9999"));

    let line_15000 = repo.read_original_line(15_000).unwrap();
    assert!(line_15000.contains("message number 15000"));

    let line_last = repo.read_original_line(24_999).unwrap();
    assert!(line_last.contains("message number 24999"));
}

#[test]
fn test_read_range() {
    let tmp = TempDir::new().unwrap();
    let repo_path = tmp.path().join("repo");

    let data = b"line0\nline1\nline2\nline3\nline4\n";
    let repo = LogRepo::import_from_bytes(&repo_path, data, "test".into()).unwrap();

    let lines = repo.read_original_lines(1, 3).unwrap();
    assert_eq!(lines, vec!["line1", "line2", "line3"]);
}

// -------- Append tests --------

#[test]
fn test_append_basic() {
    let tmp = TempDir::new().unwrap();
    let repo_path = tmp.path().join("repo");

    let data = b"a\nb\nc\n";
    let mut repo = LogRepo::import_from_bytes(&repo_path, data, "test".into()).unwrap();
    assert_eq!(repo.original_line_count(), 3);

    let added = repo.append_bytes(b"d\ne\n").unwrap();
    assert_eq!(added, 2);
    assert_eq!(repo.original_line_count(), 5);

    assert_eq!(repo.read_original_line(0).unwrap(), "a");
    assert_eq!(repo.read_original_line(3).unwrap(), "d");
    assert_eq!(repo.read_original_line(4).unwrap(), "e");
}

#[test]
fn test_append_multiple_times() {
    let tmp = TempDir::new().unwrap();
    let repo_path = tmp.path().join("repo");

    let mut repo =
        LogRepo::import_from_bytes(&repo_path, b"line0\n", "test".into()).unwrap();

    repo.append_bytes(b"line1\nline2\n").unwrap();
    repo.append_bytes(b"line3\n").unwrap();

    assert_eq!(repo.original_line_count(), 4);
    let lines = repo.read_all_original_lines().unwrap();
    assert_eq!(lines, vec!["line0", "line1", "line2", "line3"]);
}

#[test]
fn test_append_empty() {
    let tmp = TempDir::new().unwrap();
    let repo_path = tmp.path().join("repo");

    let mut repo =
        LogRepo::import_from_bytes(&repo_path, b"a\n", "test".into()).unwrap();

    let added = repo.append_bytes(b"").unwrap();
    assert_eq!(added, 0);
    assert_eq!(repo.original_line_count(), 1);
}

#[test]
fn test_append_preserves_operations() {
    let tmp = TempDir::new().unwrap();
    let repo_path = tmp.path().join("repo");

    let mut repo =
        LogRepo::import_from_bytes(&repo_path, b"INFO a\nERROR b\n", "test".into()).unwrap();

    // Apply a filter
    repo.apply_operation(Operation::Filter {
        pattern: "ERROR".to_string(),
        keep: true,
    })
    .unwrap();
    assert_eq!(repo.current_line_count().unwrap(), 1);

    // Append more data — operations re-apply over all data
    repo.append_bytes(b"INFO c\nERROR d\n").unwrap();
    assert_eq!(repo.original_line_count(), 4);

    // Current state = filter applied to all 4 lines
    let current = repo.get_current_lines().unwrap();
    assert_eq!(current, vec!["ERROR b", "ERROR d"]);
}

#[test]
fn test_append_persists_across_reopen() {
    let tmp = TempDir::new().unwrap();
    let repo_path = tmp.path().join("repo");

    {
        let mut repo =
            LogRepo::import_from_bytes(&repo_path, b"x\ny\n", "test".into()).unwrap();
        repo.append_bytes(b"z\n").unwrap();
    }

    let repo = LogRepo::open(&repo_path).unwrap();
    assert_eq!(repo.original_line_count(), 3);
    assert_eq!(repo.read_original_line(2).unwrap(), "z");
}

#[test]
fn test_append_file() {
    let tmp = TempDir::new().unwrap();
    let repo_path = tmp.path().join("repo");
    let extra_file = tmp.path().join("extra.log");

    let mut repo =
        LogRepo::import_from_bytes(&repo_path, b"first\n", "test".into()).unwrap();

    fs::write(&extra_file, "second\nthird\n").unwrap();
    let added = repo.append_file(&extra_file).unwrap();
    assert_eq!(added, 2);
    assert_eq!(repo.original_line_count(), 3);

    let lines = repo.read_all_original_lines().unwrap();
    assert_eq!(lines, vec!["first", "second", "third"]);
}

#[test]
fn test_append_large_across_chunks() {
    let tmp = TempDir::new().unwrap();
    let repo_path = tmp.path().join("repo");

    let first_batch = create_test_log(15_000);
    let mut repo =
        LogRepo::import_from_bytes(&repo_path, first_batch.as_bytes(), "batch1".into()).unwrap();
    assert_eq!(repo.original_line_count(), 15_000);

    let second_batch = create_test_log(10_000);
    let added = repo.append_bytes(second_batch.as_bytes()).unwrap();
    assert_eq!(added, 10_000);
    assert_eq!(repo.original_line_count(), 25_000);

    // Verify we can read lines from both batches
    let line_0 = repo.read_original_line(0).unwrap();
    assert!(line_0.contains("message number 0"));

    let line_14999 = repo.read_original_line(14_999).unwrap();
    assert!(line_14999.contains("message number 14999"));

    // Lines from second batch
    let line_15000 = repo.read_original_line(15_000).unwrap();
    assert!(line_15000.contains("message number 0"));

    let line_last = repo.read_original_line(24_999).unwrap();
    assert!(line_last.contains("message number 9999"));
}

#[test]
fn test_append_metadata_updated() {
    let tmp = TempDir::new().unwrap();
    let repo_path = tmp.path().join("repo");

    let data1 = b"hello\n";
    let mut repo =
        LogRepo::import_from_bytes(&repo_path, data1, "test".into()).unwrap();
    let size_before = repo.metadata.original_size;

    let data2 = b"world\n";
    repo.append_bytes(data2).unwrap();

    assert_eq!(repo.metadata.original_size, size_before + data2.len() as u64);
    assert_eq!(repo.metadata.original_line_count, 2);
}
