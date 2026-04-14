"""Tests for the Collector system — read-only terminal operations on line streams."""

import os
import tempfile

import pytest

from log_analyzer import LogRepo


@pytest.fixture
def tmp_dir():
    with tempfile.TemporaryDirectory() as d:
        yield d


def make_log(num_lines: int) -> str:
    """Generate a structured log with known distributions."""
    levels = ["INFO", "WARN", "ERROR", "DEBUG"]
    ips = ["192.168.1.1", "10.0.0.5", "172.16.0.1"]
    lines = []
    for i in range(num_lines):
        level = levels[i % 4]
        ip = ips[i % 3]
        latency = (i * 7 + 3) % 500
        lines.append(
            f"2024-01-{(i % 28) + 1:02d} [{level}] src={ip} "
            f"op=request latency={latency}ms thread-{i % 4}"
        )
    return "\n".join(lines) + "\n"


@pytest.fixture
def repo(tmp_dir):
    repo_path = os.path.join(tmp_dir, "repo")
    return LogRepo.import_text(repo_path, make_log(1200), "app.log")


# ---------- collect_count ----------

class TestCollectCount:
    def test_count_all(self, repo):
        assert repo.collect_count() == 1200

    def test_count_pattern(self, repo):
        # Every 4th line is ERROR
        assert repo.collect_count("ERROR") == 300

    def test_count_no_match(self, repo):
        assert repo.collect_count("NONEXISTENT") == 0

    def test_count_regex(self, repo):
        # Lines with thread-0 or thread-2
        count = repo.collect_count(r"thread-[02]")
        assert count == 600


# ---------- collect_group_count ----------

class TestCollectGroupCount:
    def test_group_by_level(self, repo):
        result = repo.collect_group_count(r"\[(\w+)\]", 1)
        assert result["INFO"] == 300
        assert result["WARN"] == 300
        assert result["ERROR"] == 300
        assert result["DEBUG"] == 300

    def test_group_by_ip(self, repo):
        result = repo.collect_group_count(r"src=(\S+)", 1)
        assert result["192.168.1.1"] == 400
        assert result["10.0.0.5"] == 400
        assert result["172.16.0.1"] == 400

    def test_group_no_match(self, repo):
        result = repo.collect_group_count(r"xyz=(\w+)", 1)
        assert len(result) == 0


# ---------- collect_top_n ----------

class TestCollectTopN:
    def test_top_ips(self, repo):
        result = repo.collect_top_n(r"src=(\S+)", 1, 2)
        assert len(result) == 2
        # All IPs have equal count (400), so top 2 is any 2 of 3
        for val, count in result:
            assert count == 400

    def test_top_1(self, repo):
        result = repo.collect_top_n(r"\[(\w+)\]", 1, 1)
        assert len(result) == 1
        assert result[0][1] == 300

    def test_top_n_exceeds_unique(self, repo):
        # Only 4 unique levels, but ask for top 10
        result = repo.collect_top_n(r"\[(\w+)\]", 1, 10)
        assert len(result) == 4


# ---------- collect_unique ----------

class TestCollectUnique:
    def test_unique_levels(self, repo):
        result = repo.collect_unique(r"\[(\w+)\]", 1)
        assert result == ["DEBUG", "ERROR", "INFO", "WARN"]  # sorted

    def test_unique_ips(self, repo):
        result = repo.collect_unique(r"src=(\S+)", 1)
        assert len(result) == 3
        assert "192.168.1.1" in result

    def test_unique_no_match(self, repo):
        result = repo.collect_unique(r"xyz=(\w+)", 1)
        assert result == []


# ---------- collect_numeric_stats ----------

class TestCollectNumericStats:
    def test_latency_stats(self, repo):
        result = repo.collect_numeric_stats(r"latency=(\d+)ms", 1)
        assert result["count"] == 1200
        assert result["min"] >= 0
        assert result["max"] < 500
        assert result["avg"] > 0
        assert abs(result["sum"] - result["avg"] * result["count"]) < 0.01

    def test_no_match(self, repo):
        result = repo.collect_numeric_stats(r"xyz=(\d+)", 1)
        assert result["count"] == 0
        assert result["avg"] == 0.0


# ---------- collect_line_stats ----------

class TestCollectLineStats:
    def test_line_stats(self, repo):
        result = repo.collect_line_stats()
        assert result["count"] == 1200
        assert result["total_bytes"] > 0
        assert result["avg_len"] > 0
        assert result["max_len"] >= result["min_len"]


# ---------- Collector after operations (does not modify repo) ----------

class TestCollectorAfterOperations:
    def test_collect_on_filtered_state(self, repo):
        """Collector should see the post-operation state."""
        repo.filter(r"\[ERROR\]", keep=True)
        assert repo.collect_count() == 300
        assert repo.collect_count("ERROR") == 300
        assert repo.collect_count("INFO") == 0

    def test_collect_does_not_modify_repo(self, repo):
        """Running a collector should not change the repo state."""
        lines_before = repo.read_all_lines()
        repo.collect_count()
        repo.collect_group_count(r"\[(\w+)\]", 1)
        repo.collect_top_n(r"src=(\S+)", 1, 5)
        repo.collect_unique(r"\[(\w+)\]", 1)
        repo.collect_numeric_stats(r"latency=(\d+)ms", 1)
        repo.collect_line_stats()
        lines_after = repo.read_all_lines()
        assert lines_before == lines_after

    def test_collect_after_undo(self, repo):
        """Collector result should reflect undo."""
        original = repo.collect_count()
        repo.filter("ERROR", keep=True)
        assert repo.collect_count() == 300
        repo.undo()
        assert repo.collect_count() == original


# ---------- Real-world scenario ----------

class TestCollectorScenario:
    def test_log_analysis_pipeline(self, tmp_dir):
        """Simulate a full analysis: import → filter → collect stats."""
        # Generate a web-server-like log
        lines = []
        statuses = [200, 200, 200, 200, 301, 404, 500]
        for i in range(5000):
            status = statuses[i % len(statuses)]
            ms = (i * 13 + 7) % 2000
            lines.append(f'GET /api/v1/resource HTTP/1.1 status={status} time={ms}ms')

        repo_path = os.path.join(tmp_dir, "web_repo")
        repo = LogRepo.import_text(repo_path, "\n".join(lines) + "\n", "access.log")

        # Count errors
        error_count = repo.collect_count(r"status=500")
        assert error_count > 0

        # Status distribution
        status_dist = repo.collect_group_count(r"status=(\d+)", 1)
        assert "200" in status_dist
        assert "500" in status_dist
        total = sum(status_dist.values())
        assert total == 5000

        # Top statuses
        top = repo.collect_top_n(r"status=(\d+)", 1, 3)
        assert top[0][0] == "200"  # most common

        # Response time stats
        time_stats = repo.collect_numeric_stats(r"time=(\d+)ms", 1)
        assert time_stats["count"] == 5000
        assert time_stats["min"] >= 0
        assert time_stats["max"] < 2000

        # Unique status codes
        unique_statuses = repo.collect_unique(r"status=(\d+)", 1)
        assert set(unique_statuses) == {"200", "301", "404", "500"}

        # After filter, collectors see filtered state
        repo.filter(r"status=500", keep=True)
        assert repo.collect_count() == error_count
        time_stats_errors = repo.collect_numeric_stats(r"time=(\d+)ms", 1)
        assert time_stats_errors["count"] == error_count

        # Undo filter, back to full data
        repo.undo()
        assert repo.collect_count() == 5000
