"""Tests for the Workspace API — managing multiple named repos."""

import os
import tempfile

import pytest

from log_analyzer import Workspace, LogRepo


@pytest.fixture
def tmp_dir():
    with tempfile.TemporaryDirectory() as d:
        yield d


@pytest.fixture
def ws(tmp_dir):
    return Workspace(os.path.join(tmp_dir, "ws"))


class TestWorkspaceBasic:
    def test_import_and_list(self, ws, tmp_dir):
        log_file = os.path.join(tmp_dir, "test.log")
        with open(log_file, "w") as f:
            f.write("a\nb\nc\n")

        ws.import_file(log_file, "default")
        assert ws.list() == ["default"]
        assert ws.has_repo("default")
        assert not ws.has_repo("nonexistent")

    def test_import_text(self, ws):
        ws.import_text("hello\nworld\n", "test", "myrepo")
        assert ws.has_repo("myrepo")
        repo = ws.open_repo("myrepo")
        assert repo.original_line_count() == 2

    def test_active_repo(self, ws):
        ws.import_text("a\n", "test", "first")
        assert ws.active() == "first"

        ws.import_text("b\n", "test", "second")
        assert ws.active() == "first"  # unchanged

        ws.set_active("second")
        assert ws.active() == "second"

    def test_open_active(self, ws):
        ws.import_text("x\ny\n", "test", "default")
        repo = ws.open_active()
        assert repo.original_line_count() == 2


class TestWorkspaceClone:
    def test_clone(self, ws):
        ws.import_text("line1\nline2\nline3\n", "test", "original")
        cloned = ws.clone_repo("original", "copy")
        assert cloned.original_line_count() == 3
        assert ws.has_repo("copy")
        assert sorted(ws.list()) == ["copy", "original"]

    def test_clone_duplicate_fails(self, ws):
        ws.import_text("a\n", "test", "myrepo")
        with pytest.raises(RuntimeError, match="already exists"):
            ws.clone_repo("myrepo", "myrepo")

    def test_clone_nonexistent_fails(self, ws):
        with pytest.raises(RuntimeError, match="not found"):
            ws.clone_repo("nonexistent", "copy")

    def test_cloned_repos_are_independent(self, ws):
        ws.import_text("INFO ok\nERROR bad\nINFO ok2\n", "test", "base")
        ws.clone_repo("base", "errors")

        # Filter on clone
        errors_repo = ws.open_repo("errors")
        errors_repo.filter("ERROR", keep=True)
        assert errors_repo.current_line_count() == 1

        # Original unchanged
        base_repo = ws.open_repo("base")
        assert base_repo.current_line_count() == 3


class TestWorkspaceRemove:
    def test_remove(self, ws):
        ws.import_text("a\n", "test", "to_remove")
        assert ws.has_repo("to_remove")
        ws.remove_repo("to_remove")
        assert not ws.has_repo("to_remove")

    def test_remove_nonexistent_fails(self, ws):
        with pytest.raises(RuntimeError, match="not found"):
            ws.remove_repo("nonexistent")

    def test_remove_active_switches(self, ws):
        ws.import_text("a\n", "t", "first")
        ws.import_text("b\n", "t", "second")
        ws.set_active("first")

        ws.remove_repo("first")
        assert ws.active() == "second"


class TestWorkspaceInvalidNames:
    def test_empty_name(self, ws):
        with pytest.raises(RuntimeError, match="empty"):
            ws.import_text("a\n", "test", "")

    def test_slash_in_name(self, ws):
        with pytest.raises(RuntimeError, match="Invalid"):
            ws.import_text("a\n", "test", "a/b")

    def test_dotdot_name(self, ws):
        with pytest.raises(RuntimeError, match="Invalid"):
            ws.import_text("a\n", "test", "..")


class TestWorkspaceMigration:
    def test_migrate_flat_layout(self, tmp_dir):
        """Old flat .logrepo/ layout auto-migrates to workspace."""
        ws_root = os.path.join(tmp_dir, "old_ws")

        # Create old flat layout using raw LogRepo
        LogRepo.import_text(ws_root, "old\ndata\n", "old.log")

        # Open as workspace — should auto-migrate
        ws = Workspace(ws_root)
        assert ws.has_repo("default")

        repo = ws.open_repo("default")
        assert repo.original_line_count() == 2
        assert repo.read_line(0) == "old"


class TestWorkspaceScenario:
    def test_full_workflow(self, ws, tmp_dir):
        """Simulate a real analysis workflow with multiple repos."""
        log_file = os.path.join(tmp_dir, "server.log")
        lines = [f"{'ERROR' if i % 5 == 0 else 'INFO'} message {i}" for i in range(100)]
        with open(log_file, "w") as f:
            f.write("\n".join(lines) + "\n")

        # Import
        ws.import_file(log_file, "default")
        assert ws.active() == "default"

        # Clone for error analysis
        ws.clone_repo("default", "errors")

        # Clone for info analysis
        ws.clone_repo("default", "info_only")

        assert sorted(ws.list()) == ["default", "errors", "info_only"]

        # Work on errors
        ws.set_active("errors")
        err_repo = ws.open_active()
        err_repo.filter("ERROR", keep=True)
        assert err_repo.current_line_count() == 20

        # Work on info
        info_repo = ws.open_repo("info_only")
        info_repo.filter("INFO", keep=True)
        assert info_repo.current_line_count() == 80

        # Default is untouched
        default_repo = ws.open_repo("default")
        assert default_repo.current_line_count() == 100

        # Clean up errors
        ws.remove_repo("errors")
        assert sorted(ws.list()) == ["default", "info_only"]
        assert ws.active() != "errors"
