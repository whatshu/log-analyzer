"""Integration tests simulating real-world log analysis workflows."""

import os
import tempfile

import pytest

from log_analyzer import LogRepo


def generate_web_server_log(num_lines: int) -> str:
    """Generate a realistic web server access log."""
    ips = ["192.168.1.100", "10.0.0.5", "172.16.0.1", "203.0.113.50"]
    methods = ["GET", "POST", "PUT", "DELETE"]
    paths = ["/api/users", "/api/orders", "/health", "/static/app.js", "/login"]
    statuses = [200, 200, 200, 301, 404, 500]
    lines = []
    for i in range(num_lines):
        ip = ips[i % len(ips)]
        method = methods[i % len(methods)]
        path = paths[i % len(paths)]
        status = statuses[i % len(statuses)]
        size = 100 + (i * 7) % 10000
        lines.append(
            f'{ip} - - [01/Jan/2024:{i % 24:02d}:{i % 60:02d}:{i % 60:02d} +0000] '
            f'"{method} {path} HTTP/1.1" {status} {size}'
        )
    return "\n".join(lines) + "\n"


def generate_app_log(num_lines: int) -> str:
    """Generate a realistic application log."""
    levels = ["INFO", "INFO", "INFO", "WARN", "ERROR", "DEBUG"]
    components = ["auth", "db", "api", "cache", "scheduler"]
    lines = []
    for i in range(num_lines):
        level = levels[i % len(levels)]
        component = components[i % len(components)]
        ts = f"2024-01-{(i % 28) + 1:02d}T{i % 24:02d}:{i % 60:02d}:{i % 60:02d}.{i % 1000:03d}Z"
        lines.append(f"{ts} [{level}] {component}: operation {i} completed in {i % 500}ms")
    return "\n".join(lines) + "\n"


@pytest.fixture
def tmp_dir():
    with tempfile.TemporaryDirectory() as d:
        yield d


class TestWebServerLogAnalysis:
    """Simulate analyzing a web server access log."""

    def test_full_workflow(self, tmp_dir):
        # Step 1: Generate and import a web server log
        log_content = generate_web_server_log(10_000)
        log_file = os.path.join(tmp_dir, "access.log")
        with open(log_file, "w") as f:
            f.write(log_content)

        repo_path = os.path.join(tmp_dir, "web_repo")
        repo = LogRepo.import_file(repo_path, log_file)
        assert repo.original_line_count() == 10_000

        # Step 2: Filter for errors (500 status)
        repo.filter('" 500 ', keep=True)
        error_count = repo.current_line_count()
        assert error_count > 0

        lines = repo.read_all_lines()
        assert all("500" in line for line in lines)

        # Step 3: Undo and try different analysis
        repo.undo()
        assert repo.current_line_count() == 10_000

        # Step 4: Filter for 404s
        repo.filter('" 404 ', keep=True)
        not_found_count = repo.current_line_count()
        assert not_found_count > 0

        # Step 5: Export the filtered results
        export_path = os.path.join(tmp_dir, "404_errors.log")
        repo.export(export_path)
        assert os.path.exists(export_path)

        with open(export_path) as f:
            exported_lines = f.read().strip().split("\n")
        assert len(exported_lines) == not_found_count

    def test_ip_anonymization(self, tmp_dir):
        """Replace IPs with anonymized versions."""
        log_content = generate_web_server_log(1000)
        repo_path = os.path.join(tmp_dir, "anon_repo")
        repo = LogRepo.import_text(repo_path, log_content, "access.log")

        # Replace all IP addresses
        repo.replace(r"\d{1,3}\.\d{1,3}\.\d{1,3}\.\d{1,3}", "X.X.X.X")

        lines = repo.read_all_lines()
        assert all(line.startswith("X.X.X.X") for line in lines)

        # Undo to get original
        repo.undo()
        lines = repo.read_all_lines()
        assert not any(line.startswith("X.X.X.X") for line in lines)

    def test_path_analysis(self, tmp_dir):
        """Filter and analyze specific API paths."""
        log_content = generate_web_server_log(5000)
        repo_path = os.path.join(tmp_dir, "path_repo")
        repo = LogRepo.import_text(repo_path, log_content, "access.log")

        # Keep only API requests
        repo.filter(r"/api/", keep=True)
        api_count = repo.current_line_count()
        assert api_count > 0

        lines = repo.read_all_lines()
        assert all("/api/" in line for line in lines)


class TestAppLogAnalysis:
    """Simulate analyzing an application log."""

    def test_error_investigation(self, tmp_dir):
        log_content = generate_app_log(5000)
        repo_path = os.path.join(tmp_dir, "app_repo")
        repo = LogRepo.import_text(repo_path, log_content, "app.log")

        # Step 1: Filter for errors
        repo.filter(r"\[ERROR\]", keep=True)
        error_count = repo.current_line_count()
        assert error_count > 0

        # Step 2: Further filter for specific component
        repo.filter("db:", keep=True)
        db_errors = repo.current_line_count()

        # Step 3: Check history
        history = repo.history()
        assert len(history) == 2

        # Step 4: Undo both to restore
        repo.undo()
        repo.undo()
        assert repo.current_line_count() == 5000

    def test_timestamp_normalization(self, tmp_dir):
        log_content = generate_app_log(1000)
        repo_path = os.path.join(tmp_dir, "ts_repo")
        repo = LogRepo.import_text(repo_path, log_content, "app.log")

        # Normalize timestamps to date only
        repo.replace(r"T\d{2}:\d{2}:\d{2}\.\d{3}Z", "")

        lines = repo.read_lines(0, 5)
        for line in lines:
            assert "T" not in line.split(" ")[0]

    def test_slow_query_analysis(self, tmp_dir):
        """Find all operations taking > 200ms."""
        log_content = generate_app_log(2000)
        repo_path = os.path.join(tmp_dir, "slow_repo")
        repo = LogRepo.import_text(repo_path, log_content, "app.log")

        # Filter for slow operations (3-digit ms values >= 200)
        repo.filter(r" in [2-4]\d\dms", keep=True)
        slow_count = repo.current_line_count()
        assert slow_count > 0


class TestCloneWorkflow:
    """Test branching workflows with clone."""

    def test_clone_and_diverge(self, tmp_dir):
        """Clone a repo and apply different operations to each."""
        log_content = generate_app_log(1000)
        repo_path = os.path.join(tmp_dir, "main_repo")
        repo = LogRepo.import_text(repo_path, log_content, "app.log")

        # Clone for error analysis
        error_repo_path = os.path.join(tmp_dir, "error_repo")
        error_repo = repo.clone_to(error_repo_path)

        # Clone for performance analysis
        perf_repo_path = os.path.join(tmp_dir, "perf_repo")
        perf_repo = repo.clone_to(perf_repo_path)

        # Apply different operations
        error_repo.filter(r"\[ERROR\]", keep=True)
        perf_repo.filter(r" in [2-4]\d\dms", keep=True)

        # Each repo has its own state
        error_count = error_repo.current_line_count()
        perf_count = perf_repo.current_line_count()
        assert error_count != perf_count or error_count > 0

        # Original is unchanged
        assert repo.current_line_count() == 1000


class TestEdgeCases:
    def test_empty_file(self, tmp_dir):
        repo_path = os.path.join(tmp_dir, "empty_repo")
        repo = LogRepo.import_text(repo_path, "", "empty.log")
        assert repo.original_line_count() == 0

    def test_single_line_no_newline(self, tmp_dir):
        repo_path = os.path.join(tmp_dir, "single_repo")
        repo = LogRepo.import_text(repo_path, "single line", "test")
        assert repo.original_line_count() == 1
        assert repo.read_line(0) == "single line"

    def test_unicode_content(self, tmp_dir):
        repo_path = os.path.join(tmp_dir, "unicode_repo")
        content = "日本語のログ\n中文日志\nрусский лог\n"
        repo = LogRepo.import_text(repo_path, content, "unicode.log")
        assert repo.original_line_count() == 3
        assert "日本語" in repo.read_line(0)

    def test_very_long_lines(self, tmp_dir):
        repo_path = os.path.join(tmp_dir, "long_repo")
        long_line = "x" * 100_000
        content = f"{long_line}\nshort\n"
        repo = LogRepo.import_text(repo_path, content, "test")
        assert repo.read_line(0) == long_line

    def test_filter_no_match(self, tmp_dir):
        repo_path = os.path.join(tmp_dir, "no_match_repo")
        repo = LogRepo.import_text(repo_path, "a\nb\nc\n", "test")
        repo.filter("zzz_never_matches", keep=True)
        assert repo.current_line_count() == 0

    def test_replace_no_match(self, tmp_dir):
        repo_path = os.path.join(tmp_dir, "no_match_repo")
        repo = LogRepo.import_text(repo_path, "a\nb\nc\n", "test")
        repo.replace("zzz_never_matches", "replacement")
        lines = repo.read_all_lines()
        assert lines == ["a", "b", "c"]
