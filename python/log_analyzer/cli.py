"""CLI frontend for log-analyzer."""

import os
import sys

import click
from rich.console import Console
from rich.table import Table

from log_analyzer._core import Workspace

console = Console()

DEFAULT_WORKSPACE = ".logrepo"


def get_workspace(workspace: str | None = None) -> Workspace:
    """Open workspace, auto-migrating old flat layout if needed."""
    root = workspace or DEFAULT_WORKSPACE
    return Workspace(root)


def open_repo(workspace: str | None, repo: str | None):
    """Open a named repo from the workspace."""
    ws = get_workspace(workspace)
    if not ws.is_initialized():
        console.print("[red]No workspace found. Use 'import' to create one.[/red]")
        sys.exit(1)
    name = repo or ws.active()
    return ws.open_repo(name)


# ---------------------------------------------------------------------------
# Main group
# ---------------------------------------------------------------------------

@click.group()
@click.version_option(version="0.1.0")
def main():
    """log-analyzer: High-performance log analysis tool for large text files.

    Stores logs in compressed repositories with full operation history
    and undo support. Designed for files >10GB.

    Use --repo NAME to target a specific repo (default: active repo).
    Use 'repo' subcommand to manage repos (list, clone, remove, use).
    """
    pass


# ---------------------------------------------------------------------------
# repo subcommand group
# ---------------------------------------------------------------------------

@main.group()
def repo():
    """Manage repositories (list, clone, remove, use)."""
    pass


@repo.command(name="list")
@click.option("--workspace", "-w", default=None, help="Workspace directory")
def repo_list(workspace: str | None):
    """List all repositories in the workspace."""
    ws = get_workspace(workspace)
    if not ws.is_initialized():
        console.print("[dim]No workspace found.[/dim]")
        return

    active = ws.active()
    repos = ws.list()

    if not repos:
        console.print("[dim]No repositories.[/dim]")
        return

    table = Table(title="Repositories")
    table.add_column("", style="cyan", width=3)
    table.add_column("Name", style="green")
    table.add_column("Lines", justify="right")
    table.add_column("Source")

    for name in repos:
        try:
            r = ws.open_repo(name)
            meta = r.metadata()
            marker = "*" if name == active else ""
            table.add_row(marker, name, f"{meta.original_line_count:,}", meta.source_name)
        except Exception:
            table.add_row("", name, "?", "?")

    console.print(table)
    console.print(f"\n[dim]Active: {active}[/dim]")


@repo.command(name="use")
@click.argument("name")
@click.option("--workspace", "-w", default=None, help="Workspace directory")
def repo_use(name: str, workspace: str | None):
    """Switch the active repository."""
    ws = get_workspace(workspace)
    ws.set_active(name)
    console.print(f"[green]Active repo:[/green] {name}")


@repo.command(name="clone")
@click.argument("src")
@click.argument("dst")
@click.option("--workspace", "-w", default=None, help="Workspace directory")
def repo_clone(src: str, dst: str, workspace: str | None):
    """Clone a repository under a new name."""
    ws = get_workspace(workspace)
    with console.status(f"Cloning {src} -> {dst}..."):
        ws.clone_repo(src, dst)
    console.print(f"[green]Cloned:[/green] {src} -> {dst}")


@repo.command(name="remove")
@click.argument("name")
@click.option("--workspace", "-w", default=None, help="Workspace directory")
@click.option("--yes", "-y", is_flag=True, help="Skip confirmation")
def repo_remove(name: str, workspace: str | None, yes: bool):
    """Remove a repository."""
    ws = get_workspace(workspace)
    if not yes:
        click.confirm(f"Remove repo '{name}'? This cannot be undone", abort=True)
    ws.remove_repo(name)
    console.print(f"[green]Removed:[/green] {name}")


# ---------------------------------------------------------------------------
# Top-level log commands (operate on the active or --repo repo)
# ---------------------------------------------------------------------------

@main.command(name="import")
@click.argument("file", type=click.Path(exists=True))
@click.option("--repo", "-r", default="default", help="Repository name")
@click.option("--workspace", "-w", default=None, help="Workspace directory")
def import_cmd(file: str, repo: str, workspace: str | None):
    """Import a text file into a new repository."""
    ws = get_workspace(workspace)
    with console.status("Importing log file..."):
        log_repo = ws.import_file(file, repo)

    meta = log_repo.metadata()
    console.print(f"[green]Repo '{repo}' created[/green]")
    console.print(f"  Source: {meta.source_name}")
    console.print(f"  Lines:  {meta.original_line_count:,}")
    console.print(f"  Size:   {_format_size(meta.original_size)}")


@main.command()
@click.argument("file", type=click.Path(exists=True))
@click.option("--repo", "-r", default=None, help="Repository name")
@click.option("--workspace", "-w", default=None, help="Workspace directory")
def append(file: str, repo: str | None, workspace: str | None):
    """Append a text file into an existing repository."""
    log_repo = open_repo(workspace, repo)

    before = log_repo.original_line_count()
    with console.status("Appending log file..."):
        added = log_repo.append_file(file)
    after = log_repo.original_line_count()

    console.print(f"[green]Appended:[/green] {file}")
    console.print(f"  New lines: {added:,}")
    console.print(f"  Total:     {before:,} -> {after:,}")


@main.command()
@click.option("--repo", "-r", default=None, help="Repository name")
@click.option("--workspace", "-w", default=None, help="Workspace directory")
def info(repo: str | None, workspace: str | None):
    """Show repository information."""
    ws = get_workspace(workspace)
    name = repo or ws.active()
    log_repo = ws.open_repo(name)
    meta = log_repo.metadata()
    history = log_repo.history()

    table = Table(title=f"Repository: {name}")
    table.add_column("Property", style="cyan")
    table.add_column("Value", style="green")

    table.add_row("ID", meta.id)
    table.add_row("Source", meta.source_name)
    table.add_row("Original Lines", f"{meta.original_line_count:,}")
    table.add_row("Original Size", _format_size(meta.original_size))
    table.add_row("Current Lines", f"{log_repo.current_line_count():,}")
    table.add_row("Operations", str(len(history)))
    table.add_row("Created", meta.created_at)

    console.print(table)


@main.command()
@click.option("--repo", "-r", default=None, help="Repository name")
@click.option("--workspace", "-w", default=None, help="Workspace directory")
@click.option("--start", "-s", default=0, help="Start line number (0-based)")
@click.option("--count", "-n", default=20, help="Number of lines to show")
@click.option("--numbers/--no-numbers", default=True, help="Show line numbers")
def view(repo: str | None, workspace: str | None, start: int, count: int, numbers: bool):
    """View lines from the current state of the log."""
    log_repo = open_repo(workspace, repo)
    lines = log_repo.read_lines(start, count)
    total = log_repo.current_line_count()

    if numbers:
        width = len(str(start + len(lines)))
        for i, line in enumerate(lines):
            line_num = start + i
            console.print(f"[dim]{line_num:>{width}}[/dim] {line}")
    else:
        for line in lines:
            console.print(line)

    console.print(f"\n[dim]Showing lines {start}-{start + len(lines) - 1} of {total:,}[/dim]")


@main.command()
@click.argument("pattern")
@click.option("--keep/--remove", default=True, help="Keep or remove matching lines")
@click.option("--repo", "-r", default=None, help="Repository name")
@click.option("--workspace", "-w", default=None, help="Workspace directory")
def filter(pattern: str, keep: bool, repo: str | None, workspace: str | None):
    """Filter lines by regex pattern.

    PATTERN is a regular expression. Use --keep (default) to keep matching
    lines, or --remove to remove them.
    """
    log_repo = open_repo(workspace, repo)

    before = log_repo.current_line_count()
    log_repo.filter(pattern, keep)
    after = log_repo.current_line_count()

    action = "kept" if keep else "removed"
    diff = abs(after - before)
    console.print(f"[green]Filter applied:[/green] {action} {diff:,} lines (/{pattern}/)")
    console.print(f"  Lines: {before:,} -> {after:,}")


@main.command()
@click.argument("pattern")
@click.argument("replacement")
@click.option("--repo", "-r", default=None, help="Repository name")
@click.option("--workspace", "-w", default=None, help="Workspace directory")
def replace(pattern: str, replacement: str, repo: str | None, workspace: str | None):
    """Replace text matching a regex pattern.

    PATTERN is a regular expression. REPLACEMENT can include capture
    group references like $1, $2, etc.
    """
    log_repo = open_repo(workspace, repo)
    log_repo.replace(pattern, replacement)
    console.print(f"[green]Replace applied:[/green] /{pattern}/ -> \"{replacement}\"")


@main.command()
@click.argument("indices", nargs=-1, type=int, required=True)
@click.option("--repo", "-r", default=None, help="Repository name")
@click.option("--workspace", "-w", default=None, help="Workspace directory")
def delete(indices: tuple[int, ...], repo: str | None, workspace: str | None):
    """Delete specific lines by their indices (0-based)."""
    log_repo = open_repo(workspace, repo)
    log_repo.delete_lines(list(indices))
    console.print(f"[green]Deleted {len(indices)} line(s)[/green]")


@main.command()
@click.argument("after_line", type=int)
@click.argument("content", nargs=-1, required=True)
@click.option("--repo", "-r", default=None, help="Repository name")
@click.option("--workspace", "-w", default=None, help="Workspace directory")
def insert(after_line: int, content: tuple[str, ...], repo: str | None, workspace: str | None):
    """Insert lines after the specified position.

    AFTER_LINE is the position to insert after (0 = insert at beginning).
    """
    log_repo = open_repo(workspace, repo)
    log_repo.insert_lines(after_line, list(content))
    console.print(f"[green]Inserted {len(content)} line(s) after line {after_line}[/green]")


@main.command()
@click.argument("line_index", type=int)
@click.argument("new_content")
@click.option("--repo", "-r", default=None, help="Repository name")
@click.option("--workspace", "-w", default=None, help="Workspace directory")
def modify(line_index: int, new_content: str, repo: str | None, workspace: str | None):
    """Modify a single line by its index (0-based)."""
    log_repo = open_repo(workspace, repo)
    log_repo.modify_line(line_index, new_content)
    console.print(f"[green]Modified line {line_index}[/green]")


@main.command()
@click.option("--repo", "-r", default=None, help="Repository name")
@click.option("--workspace", "-w", default=None, help="Workspace directory")
def undo(repo: str | None, workspace: str | None):
    """Undo the last operation."""
    log_repo = open_repo(workspace, repo)
    desc = log_repo.undo()
    console.print(f"[green]Undone:[/green] {desc}")


@main.command()
@click.option("--repo", "-r", default=None, help="Repository name")
@click.option("--workspace", "-w", default=None, help="Workspace directory")
def history(repo: str | None, workspace: str | None):
    """Show operation history."""
    log_repo = open_repo(workspace, repo)

    records = log_repo.history()
    if not records:
        console.print("[dim]No operations applied yet.[/dim]")
        return

    table = Table(title="Operation History")
    table.add_column("ID", style="cyan")
    table.add_column("Operation", style="green")
    table.add_column("Applied At", style="dim")

    for record in records:
        table.add_row(str(record.id), record.description, record.applied_at)

    console.print(table)


@main.command()
@click.argument("dest", type=click.Path())
@click.option("--repo", "-r", default=None, help="Repository name")
@click.option("--workspace", "-w", default=None, help="Workspace directory")
def export(dest: str, repo: str | None, workspace: str | None):
    """Export current state to a text file."""
    log_repo = open_repo(workspace, repo)

    with console.status("Exporting..."):
        log_repo.export(dest)

    console.print(f"[green]Exported to:[/green] {dest}")


@main.command()
@click.argument("pattern")
@click.option("--repo", "-r", default=None, help="Repository name")
@click.option("--workspace", "-w", default=None, help="Workspace directory")
@click.option("--count", "-n", default=20, help="Max results to show")
def search(pattern: str, repo: str | None, workspace: str | None, count: int):
    """Search for lines matching a regex pattern (read-only, no modification)."""
    log_repo = open_repo(workspace, repo)

    results = log_repo.stream_search(pattern, count)
    if not results:
        console.print(f"[dim]No matches found for /{pattern}/[/dim]")
        return

    for line_num, content in results:
        console.print(f"[dim]{line_num:>8}[/dim] {content}")

    console.print(f"\n[green]{len(results)} match(es) shown[/green]")


def _format_size(size_bytes: int) -> str:
    """Format byte size to human-readable string."""
    for unit in ["B", "KB", "MB", "GB", "TB"]:
        if size_bytes < 1024:
            return f"{size_bytes:.1f} {unit}"
        size_bytes /= 1024
    return f"{size_bytes:.1f} PB"


if __name__ == "__main__":
    main()
