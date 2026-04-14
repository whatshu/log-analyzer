"""Tests for the streaming engine (memory-efficient large file processing)."""

import os
import tempfile

import pytest

from log_analyzer import LogRepo


@pytest.fixture
def tmp_dir():
    with tempfile.TemporaryDirectory() as d:
        yield d


def make_log(num_lines: int) -> str:
    lines = []
    for i in range(num_lines):
        level = ["INFO", "WARN", "ERROR", "DEBUG"][i % 4]
        lines.append(f"2024-01-{(i % 28) + 1:02d} {level} [thread-{i % 4}] message {i}")
    return "\n".join(lines) + "\n"


@pytest.fixture
def repo(tmp_dir):
    repo_path = os.path.join(tmp_dir, "repo")
    content = make_log(1000)
    return LogRepo.import_text(repo_path, content, "test.log")


class TestCountMatches:
    def test_count_errors(self, repo):
        count = repo.count_matches("ERROR")
        assert count == 250  # every 4th line

    def test_count_no_match(self, repo):
        count = repo.count_matches("NONEXISTENT")
        assert count == 0

    def test_count_regex(self, repo):
        count = repo.count_matches(r"thread-[02]")
        assert count == 500


class TestStreamFilterToFile:
    def test_filter_keep(self, repo, tmp_dir):
        output = os.path.join(tmp_dir, "errors.log")
        written = repo.stream_filter_to_file("ERROR", True, output)
        assert written == 250
        with open(output) as f:
            lines = f.read().strip().split("\n")
        assert len(lines) == 250
        assert all("ERROR" in l for l in lines)

    def test_filter_remove(self, repo, tmp_dir):
        output = os.path.join(tmp_dir, "no_debug.log")
        written = repo.stream_filter_to_file("DEBUG", False, output)
        assert written == 750
        with open(output) as f:
            lines = f.read().strip().split("\n")
        assert all("DEBUG" not in l for l in lines)


class TestStreamReplaceToFile:
    def test_replace(self, repo, tmp_dir):
        output = os.path.join(tmp_dir, "replaced.log")
        modified = repo.stream_replace_to_file("ERROR", "CRITICAL", output)
        assert modified == 250
        with open(output) as f:
            content = f.read()
        assert "CRITICAL" in content
        assert "ERROR" not in content


class TestStreamSearch:
    def test_search(self, repo):
        results = repo.stream_search("ERROR", 10)
        assert len(results) == 10
        for line_num, content in results:
            assert "ERROR" in content

    def test_search_limit(self, repo):
        results = repo.stream_search("ERROR", 5)
        assert len(results) == 5

    def test_search_no_match(self, repo):
        results = repo.stream_search("NONEXISTENT", 10)
        assert len(results) == 0


class TestParallelSearch:
    def test_parallel_search(self, repo):
        results = repo.parallel_search("ERROR", 50)
        assert len(results) == 50
        # Results should be sorted by line number
        for i in range(len(results) - 1):
            assert results[i][0] < results[i + 1][0]

    def test_parallel_search_all(self, repo):
        results = repo.parallel_search("ERROR", 1000)
        assert len(results) == 250


class TestStreamExport:
    def test_export(self, repo, tmp_dir):
        output = os.path.join(tmp_dir, "full.log")
        count = repo.stream_export(output)
        assert count == 1000
        with open(output) as f:
            lines = f.read().strip().split("\n")
        assert len(lines) == 1000


class TestStats:
    def test_stats(self, repo):
        stats = repo.stats()
        assert stats.total_lines == 1000
        assert stats.total_bytes > 0
        assert stats.avg_line_len > 0
        assert stats.max_line_len >= stats.min_line_len
        assert stats.chunk_count > 0

    def test_stats_repr(self, repo):
        stats = repo.stats()
        r = repr(stats)
        assert "lines=1000" in r
