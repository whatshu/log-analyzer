"""Tests for the append functionality."""

import os
import tempfile

import pytest

from log_analyzer import LogRepo


@pytest.fixture
def tmp_dir():
    with tempfile.TemporaryDirectory() as d:
        yield d


class TestAppendText:
    def test_append_basic(self, tmp_dir):
        repo_path = os.path.join(tmp_dir, "repo")
        repo = LogRepo.import_text(repo_path, "a\nb\nc\n", "test")
        assert repo.original_line_count() == 3

        added = repo.append_text("d\ne\n")
        assert added == 2
        assert repo.original_line_count() == 5

        lines = repo.read_all_lines()
        assert lines == ["a", "b", "c", "d", "e"]

    def test_append_multiple_times(self, tmp_dir):
        repo_path = os.path.join(tmp_dir, "repo")
        repo = LogRepo.import_text(repo_path, "first\n", "test")

        repo.append_text("second\n")
        repo.append_text("third\nfourth\n")

        assert repo.original_line_count() == 4
        lines = repo.read_all_lines()
        assert lines == ["first", "second", "third", "fourth"]

    def test_append_empty(self, tmp_dir):
        repo_path = os.path.join(tmp_dir, "repo")
        repo = LogRepo.import_text(repo_path, "a\n", "test")

        added = repo.append_text("")
        assert added == 0
        assert repo.original_line_count() == 1


class TestAppendFile:
    def test_append_file(self, tmp_dir):
        repo_path = os.path.join(tmp_dir, "repo")
        extra_file = os.path.join(tmp_dir, "extra.log")

        repo = LogRepo.import_text(repo_path, "line1\n", "test")
        with open(extra_file, "w") as f:
            f.write("line2\nline3\n")

        added = repo.append_file(extra_file)
        assert added == 2
        assert repo.original_line_count() == 3
        assert repo.read_all_lines() == ["line1", "line2", "line3"]

    def test_append_multiple_files(self, tmp_dir):
        """Simulate concatenating multiple log files into one repo."""
        repo_path = os.path.join(tmp_dir, "repo")
        files = []
        for i in range(3):
            path = os.path.join(tmp_dir, f"part{i}.log")
            with open(path, "w") as f:
                for j in range(100):
                    f.write(f"[part{i}] line {j}\n")
            files.append(path)

        # Import first file
        repo = LogRepo.import_file(repo_path, files[0])
        assert repo.original_line_count() == 100

        # Append remaining files
        for path in files[1:]:
            repo.append_file(path)

        assert repo.original_line_count() == 300

        # Verify data from all parts is accessible
        lines = repo.read_lines(0, 1)
        assert "[part0]" in lines[0]

        lines = repo.read_lines(100, 1)
        assert "[part1]" in lines[0]

        lines = repo.read_lines(200, 1)
        assert "[part2]" in lines[0]


class TestAppendWithOperations:
    def test_operations_reapply_after_append(self, tmp_dir):
        repo_path = os.path.join(tmp_dir, "repo")
        repo = LogRepo.import_text(
            repo_path, "INFO ok\nERROR fail\n", "test"
        )

        # Apply filter
        repo.filter("ERROR", keep=True)
        assert repo.current_line_count() == 1

        # Append more data — filter should re-apply to full data
        repo.append_text("INFO another\nERROR boom\n")
        assert repo.original_line_count() == 4

        # Current view: filter applies to all 4 lines
        lines = repo.read_all_lines()
        assert lines == ["ERROR fail", "ERROR boom"]

    def test_undo_still_works_after_append(self, tmp_dir):
        repo_path = os.path.join(tmp_dir, "repo")
        repo = LogRepo.import_text(repo_path, "a\nb\n", "test")
        repo.filter("a", keep=True)
        assert repo.current_line_count() == 1

        repo.append_text("c\na2\n")
        # filter("a") on [a, b, c, a2] → [a, a2]
        lines = repo.read_all_lines()
        assert lines == ["a", "a2"]

        repo.undo()
        assert repo.current_line_count() == 4
        assert repo.read_all_lines() == ["a", "b", "c", "a2"]


class TestAppendPersistence:
    def test_persists_across_reopen(self, tmp_dir):
        repo_path = os.path.join(tmp_dir, "repo")
        repo = LogRepo.import_text(repo_path, "x\ny\n", "test")
        repo.append_text("z\n")
        del repo

        repo = LogRepo.open(repo_path)
        assert repo.original_line_count() == 3
        assert repo.read_all_lines() == ["x", "y", "z"]

    def test_metadata_updated(self, tmp_dir):
        repo_path = os.path.join(tmp_dir, "repo")
        repo = LogRepo.import_text(repo_path, "hi\n", "test")

        meta_before = repo.metadata()
        size_before = meta_before.original_size

        repo.append_text("world\n")
        meta_after = repo.metadata()

        assert meta_after.original_size > size_before
        assert meta_after.original_line_count == 2


class TestAppendCollector:
    def test_collector_sees_appended_data(self, tmp_dir):
        repo_path = os.path.join(tmp_dir, "repo")
        repo = LogRepo.import_text(
            repo_path,
            "[INFO] a\n[ERROR] b\n",
            "test",
        )
        assert repo.collect_count() == 2

        repo.append_text("[WARN] c\n[ERROR] d\n")
        assert repo.collect_count() == 4
        assert repo.collect_count("ERROR") == 2

        gc = repo.collect_group_count(r"\[(\w+)\]", 1)
        assert gc["INFO"] == 1
        assert gc["ERROR"] == 2
        assert gc["WARN"] == 1
