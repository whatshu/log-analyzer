"""Tests for the log_analyzer Python API."""

import os
import tempfile

import pytest

from log_analyzer import LogRepo


@pytest.fixture
def tmp_dir():
    with tempfile.TemporaryDirectory() as d:
        yield d


@pytest.fixture
def sample_log(tmp_dir):
    path = os.path.join(tmp_dir, "sample.log")
    lines = []
    for i in range(100):
        level = ["INFO", "WARN", "ERROR", "DEBUG"][i % 4]
        lines.append(f"2024-01-{(i % 28) + 1:02d} {level} [thread-{i % 4}] message {i}")
    with open(path, "w") as f:
        f.write("\n".join(lines) + "\n")
    return path


@pytest.fixture
def repo(tmp_dir, sample_log):
    repo_path = os.path.join(tmp_dir, "repo")
    return LogRepo.import_file(repo_path, sample_log)


class TestImport:
    def test_import_file(self, repo):
        assert repo.original_line_count() == 100

    def test_import_text(self, tmp_dir):
        repo_path = os.path.join(tmp_dir, "text_repo")
        repo = LogRepo.import_text(repo_path, "line1\nline2\nline3\n", "test")
        assert repo.original_line_count() == 3

    def test_import_duplicate_fails(self, tmp_dir, sample_log):
        repo_path = os.path.join(tmp_dir, "dup_repo")
        LogRepo.import_file(repo_path, sample_log)
        with pytest.raises(RuntimeError, match="already exists"):
            LogRepo.import_file(repo_path, sample_log)

    def test_metadata(self, repo):
        meta = repo.metadata()
        assert meta.original_line_count == 100
        assert meta.original_size > 0
        assert meta.id
        assert meta.created_at


class TestReadLines:
    def test_read_line(self, repo):
        line = repo.read_line(0)
        assert "message 0" in line

    def test_read_lines_range(self, repo):
        lines = repo.read_lines(0, 5)
        assert len(lines) == 5
        for i, line in enumerate(lines):
            assert f"message {i}" in line

    def test_read_all_lines(self, repo):
        lines = repo.read_all_lines()
        assert len(lines) == 100

    def test_read_out_of_range(self, repo):
        with pytest.raises(RuntimeError):
            repo.read_lines(1000, 1)


class TestFilter:
    def test_filter_keep(self, repo):
        repo.filter("ERROR", keep=True)
        count = repo.current_line_count()
        assert count == 25  # every 4th line
        lines = repo.read_all_lines()
        assert all("ERROR" in line for line in lines)

    def test_filter_remove(self, repo):
        repo.filter("DEBUG", keep=False)
        count = repo.current_line_count()
        assert count == 75
        lines = repo.read_all_lines()
        assert all("DEBUG" not in line for line in lines)

    def test_filter_regex(self, repo):
        repo.filter(r"thread-[02]", keep=True)
        lines = repo.read_all_lines()
        for line in lines:
            assert "thread-0" in line or "thread-2" in line


class TestReplace:
    def test_replace_simple(self, repo):
        repo.replace("ERROR", "CRITICAL")
        lines = repo.read_all_lines()
        assert all("ERROR" not in line for line in lines)
        critical_lines = [l for l in lines if "CRITICAL" in l]
        assert len(critical_lines) == 25

    def test_replace_regex_groups(self, tmp_dir):
        repo_path = os.path.join(tmp_dir, "regex_repo")
        repo = LogRepo.import_text(repo_path, "user=alice\nuser=bob\n", "test")
        repo.replace(r"user=(\w+)", r"user=[$1]")
        lines = repo.read_all_lines()
        assert lines[0] == "user=[alice]"
        assert lines[1] == "user=[bob]"


class TestCRUD:
    def test_delete_lines(self, tmp_dir):
        repo_path = os.path.join(tmp_dir, "crud_repo")
        repo = LogRepo.import_text(repo_path, "a\nb\nc\nd\ne\n", "test")
        repo.delete_lines([1, 3])
        lines = repo.read_all_lines()
        assert lines == ["a", "c", "e"]

    def test_insert_lines(self, tmp_dir):
        repo_path = os.path.join(tmp_dir, "insert_repo")
        repo = LogRepo.import_text(repo_path, "a\nb\nc\n", "test")
        repo.insert_lines(1, ["x", "y"])
        lines = repo.read_all_lines()
        assert lines == ["a", "x", "y", "b", "c"]

    def test_modify_line(self, tmp_dir):
        repo_path = os.path.join(tmp_dir, "modify_repo")
        repo = LogRepo.import_text(repo_path, "a\nb\nc\n", "test")
        repo.modify_line(1, "modified")
        lines = repo.read_all_lines()
        assert lines == ["a", "modified", "c"]


class TestUndo:
    def test_undo_filter(self, repo):
        original_count = repo.original_line_count()
        repo.filter("ERROR", keep=True)
        assert repo.current_line_count() == 25
        desc = repo.undo()
        assert "filter" in desc
        assert repo.current_line_count() == original_count

    def test_undo_replace(self, tmp_dir):
        repo_path = os.path.join(tmp_dir, "undo_repo")
        repo = LogRepo.import_text(repo_path, "hello\nworld\n", "test")
        repo.replace("hello", "HI")
        assert repo.read_line(0) == "HI"
        repo.undo()
        assert repo.read_line(0) == "hello"

    def test_undo_delete(self, tmp_dir):
        repo_path = os.path.join(tmp_dir, "undo_del_repo")
        repo = LogRepo.import_text(repo_path, "a\nb\nc\n", "test")
        repo.delete_lines([1])
        assert repo.current_line_count() == 2
        repo.undo()
        assert repo.current_line_count() == 3
        assert repo.read_all_lines() == ["a", "b", "c"]

    def test_undo_insert(self, tmp_dir):
        repo_path = os.path.join(tmp_dir, "undo_ins_repo")
        repo = LogRepo.import_text(repo_path, "a\nb\n", "test")
        repo.insert_lines(1, ["x"])
        assert repo.current_line_count() == 3
        repo.undo()
        assert repo.current_line_count() == 2
        assert repo.read_all_lines() == ["a", "b"]

    def test_undo_modify(self, tmp_dir):
        repo_path = os.path.join(tmp_dir, "undo_mod_repo")
        repo = LogRepo.import_text(repo_path, "a\nb\n", "test")
        repo.modify_line(0, "changed")
        assert repo.read_line(0) == "changed"
        repo.undo()
        assert repo.read_line(0) == "a"

    def test_undo_empty_fails(self, tmp_dir):
        repo_path = os.path.join(tmp_dir, "undo_empty_repo")
        repo = LogRepo.import_text(repo_path, "a\n", "test")
        with pytest.raises(RuntimeError, match="No operations"):
            repo.undo()

    def test_multiple_undo(self, repo):
        repo.filter("ERROR", keep=True)
        repo.replace("ERROR", "ERR")
        assert repo.current_line_count() == 25

        repo.undo()  # undo replace
        lines = repo.read_all_lines()
        assert all("ERROR" in l for l in lines)

        repo.undo()  # undo filter
        assert repo.current_line_count() == 100


class TestHistory:
    def test_empty_history(self, tmp_dir):
        repo_path = os.path.join(tmp_dir, "hist_repo")
        repo = LogRepo.import_text(repo_path, "a\n", "test")
        assert len(repo.history()) == 0

    def test_history_records(self, repo):
        repo.filter("ERROR", keep=True)
        repo.replace("ERROR", "ERR")

        history = repo.history()
        assert len(history) == 2
        assert "filter" in history[0].description
        assert "replace" in history[1].description
        assert history[0].applied_at
        assert history[1].applied_at


class TestCloneAndExport:
    def test_clone(self, repo, tmp_dir):
        clone_path = os.path.join(tmp_dir, "clone")
        cloned = repo.clone_to(clone_path)
        assert cloned.original_line_count() == repo.original_line_count()
        assert cloned.read_line(0) == repo.read_line(0)

    def test_export(self, repo, tmp_dir):
        repo.filter("ERROR", keep=True)
        export_path = os.path.join(tmp_dir, "exported.log")
        repo.export(export_path)

        with open(export_path) as f:
            content = f.read()
        lines = content.strip().split("\n")
        assert len(lines) == 25
        assert all("ERROR" in l for l in lines)


class TestPersistence:
    def test_operations_persist(self, tmp_dir, sample_log):
        repo_path = os.path.join(tmp_dir, "persist_repo")

        # Create and apply operations
        repo = LogRepo.import_file(repo_path, sample_log)
        repo.filter("ERROR", keep=True)
        del repo

        # Reopen and verify
        repo = LogRepo.open(repo_path)
        assert len(repo.history()) == 1
        assert repo.current_line_count() == 25
