use std::fs;
use tempfile::TempDir;

use log_analyzer_core::operator::Operation;
use log_analyzer_core::repo::{LogRepo, Workspace};

#[test]
fn test_workspace_import_and_list() {
    let tmp = TempDir::new().unwrap();
    let ws = Workspace::open(tmp.path()).unwrap();

    let log_file = tmp.path().join("test.log");
    fs::write(&log_file, "a\nb\nc\n").unwrap();

    ws.import_file("default", &log_file).unwrap();

    let repos = ws.list().unwrap();
    assert_eq!(repos, vec!["default"]);
    assert!(ws.has_repo("default"));
    assert!(!ws.has_repo("nonexistent"));
}

#[test]
fn test_workspace_active_repo() {
    let tmp = TempDir::new().unwrap();
    let ws = Workspace::open(tmp.path()).unwrap();

    let log_file = tmp.path().join("test.log");
    fs::write(&log_file, "a\nb\n").unwrap();

    ws.import_file("default", &log_file).unwrap();
    assert_eq!(ws.active().unwrap(), "default");

    ws.import_file("second", &log_file).unwrap();
    // First repo remains active
    assert_eq!(ws.active().unwrap(), "default");

    ws.set_active("second").unwrap();
    assert_eq!(ws.active().unwrap(), "second");
}

#[test]
fn test_workspace_clone_repo() {
    let tmp = TempDir::new().unwrap();
    let ws = Workspace::open(tmp.path()).unwrap();

    let log_file = tmp.path().join("test.log");
    fs::write(&log_file, "line1\nline2\nline3\n").unwrap();

    ws.import_file("original", &log_file).unwrap();
    let cloned = ws.clone_repo("original", "copy").unwrap();

    assert_eq!(cloned.original_line_count(), 3);
    assert!(ws.has_repo("copy"));

    let repos = ws.list().unwrap();
    assert!(repos.contains(&"original".to_string()));
    assert!(repos.contains(&"copy".to_string()));
}

#[test]
fn test_workspace_clone_duplicate_name_fails() {
    let tmp = TempDir::new().unwrap();
    let ws = Workspace::open(tmp.path()).unwrap();

    let log_file = tmp.path().join("test.log");
    fs::write(&log_file, "a\n").unwrap();

    ws.import_file("myrepo", &log_file).unwrap();
    assert!(ws.clone_repo("myrepo", "myrepo").is_err());
}

#[test]
fn test_workspace_clone_nonexistent_src_fails() {
    let tmp = TempDir::new().unwrap();
    let ws = Workspace::open(tmp.path()).unwrap();

    assert!(ws.clone_repo("nonexistent", "copy").is_err());
}

#[test]
fn test_workspace_remove_repo() {
    let tmp = TempDir::new().unwrap();
    let ws = Workspace::open(tmp.path()).unwrap();

    let log_file = tmp.path().join("test.log");
    fs::write(&log_file, "a\n").unwrap();

    ws.import_file("to_remove", &log_file).unwrap();
    assert!(ws.has_repo("to_remove"));

    ws.remove_repo("to_remove").unwrap();
    assert!(!ws.has_repo("to_remove"));
    assert!(ws.list().unwrap().is_empty());
}

#[test]
fn test_workspace_remove_active_switches() {
    let tmp = TempDir::new().unwrap();
    let ws = Workspace::open(tmp.path()).unwrap();

    let log_file = tmp.path().join("test.log");
    fs::write(&log_file, "a\n").unwrap();

    ws.import_file("first", &log_file).unwrap();
    ws.import_file("second", &log_file).unwrap();
    ws.set_active("first").unwrap();

    ws.remove_repo("first").unwrap();
    // Active should switch to remaining repo
    let active = ws.active().unwrap();
    assert_eq!(active, "second");
}

#[test]
fn test_workspace_open_repo() {
    let tmp = TempDir::new().unwrap();
    let ws = Workspace::open(tmp.path()).unwrap();

    let log_file = tmp.path().join("test.log");
    fs::write(&log_file, "hello\nworld\n").unwrap();

    ws.import_file("myrepo", &log_file).unwrap();

    let repo = ws.open_repo("myrepo").unwrap();
    assert_eq!(repo.original_line_count(), 2);
    assert_eq!(repo.read_original_line(0).unwrap(), "hello");
}

#[test]
fn test_workspace_open_active() {
    let tmp = TempDir::new().unwrap();
    let ws = Workspace::open(tmp.path()).unwrap();

    let log_file = tmp.path().join("test.log");
    fs::write(&log_file, "x\ny\n").unwrap();

    ws.import_file("default", &log_file).unwrap();
    let repo = ws.open_active().unwrap();
    assert_eq!(repo.original_line_count(), 2);
}

#[test]
fn test_workspace_repos_are_independent() {
    let tmp = TempDir::new().unwrap();
    let ws = Workspace::open(tmp.path()).unwrap();

    let log_file = tmp.path().join("test.log");
    fs::write(&log_file, "INFO ok\nERROR bad\nINFO ok2\n").unwrap();

    ws.import_file("base", &log_file).unwrap();
    ws.clone_repo("base", "errors").unwrap();

    // Apply filter to cloned repo
    let mut errors_repo = ws.open_repo("errors").unwrap();
    errors_repo
        .apply_operation(Operation::Filter {
            pattern: "ERROR".to_string(),
            keep: true,
        })
        .unwrap();
    assert_eq!(errors_repo.current_line_count().unwrap(), 1);

    // Original is unchanged
    let mut base_repo = ws.open_repo("base").unwrap();
    assert_eq!(base_repo.current_line_count().unwrap(), 3);
}

#[test]
fn test_workspace_import_bytes() {
    let tmp = TempDir::new().unwrap();
    let ws = Workspace::open(tmp.path()).unwrap();

    ws.import_bytes("test", b"foo\nbar\n", "inline".to_string())
        .unwrap();

    let repo = ws.open_repo("test").unwrap();
    assert_eq!(repo.original_line_count(), 2);
}

#[test]
fn test_workspace_invalid_names() {
    let tmp = TempDir::new().unwrap();
    let ws = Workspace::open(tmp.path()).unwrap();

    assert!(ws.import_bytes("", b"a\n", "test".to_string()).is_err());
    assert!(ws
        .import_bytes("a/b", b"a\n", "test".to_string())
        .is_err());
    assert!(ws.import_bytes("..", b"a\n", "test".to_string()).is_err());
}

#[test]
fn test_workspace_migrate_flat_layout() {
    let tmp = TempDir::new().unwrap();
    let ws_root = tmp.path().join("ws");

    // Create old flat layout
    let old_repo = LogRepo::import_from_bytes(&ws_root, b"old\ndata\n", "old.log".to_string()).unwrap();
    drop(old_repo);

    // Open as workspace — should auto-migrate
    let ws = Workspace::open(&ws_root).unwrap();
    let migrated = ws.migrate_if_needed().unwrap();
    assert!(migrated);

    // Now should work as a workspace
    assert!(ws.has_repo("default"));
    let repo = ws.open_repo("default").unwrap();
    assert_eq!(repo.original_line_count(), 2);
}
