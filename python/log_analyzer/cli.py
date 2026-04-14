"""CLI frontend for log-analyzer."""

import os
import sys

import click
from rich.console import Console
from rich.table import Table

from log_analyzer._core import LogRepo

console = Console()

DEFAULT_REPO_DIR = ".logrepo"


def get_repo_path(repo: str | None) -> str:
    """Resolve repository path."""
    if repo:
        return repo
    if os.path.isdir(DEFAULT_REPO_DIR):
        return DEFAULT_REPO_DIR
    console.print("[red]No repository found. Use 'import' to create one, or specify --repo.[/red]")
    sys.exit(1)


@click.group()
@click.version_option(version="0.1.0")
def main():
    """log-analyzer: High-performance log analysis tool for large text files.

    Stores logs in compressed repositories with full operation history
    and undo support. Designed for files >10GB.
    """
    pass


@main.command(name="import")
@click.argument("file", type=click.Path(exists=True))
@click.option("--repo", "-r", default=DEFAULT_REPO_DIR, help="Repository path")
def import_cmd(file: str, repo: str):
    """Import a text file into a new log repository."""
    if os.path.exists(repo):
        console.print(f"[red]Repository already exists: {repo}[/red]")
        console.print("Use --repo to specify a different path.")
        sys.exit(1)

    with console.status("Importing log file..."):
        log_repo = LogRepo.import_file(repo, file)

    meta = log_repo.metadata()
    console.print(f"[green]Repository created:[/green] {repo}")
    console.print(f"  Source: {meta.source_name}")
    console.print(f"  Lines:  {meta.original_line_count:,}")
    console.print(f"  Size:   {_format_size(meta.original_size)}")


@main.command()
@click.option("--repo", "-r", default=None, help="Repository path")
def info(repo: str | None):
    """Show repository information."""
    repo_path = get_repo_path(repo)
    log_repo = LogRepo.open(repo_path)
    meta = log_repo.metadata()
    history = log_repo.history()

    table = Table(title="Repository Info")
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
@click.option("--repo", "-r", default=None, help="Repository path")
@click.option("--start", "-s", default=0, help="Start line number (0-based)")
@click.option("--count", "-n", default=20, help="Number of lines to show")
@click.option("--numbers/--no-numbers", default=True, help="Show line numbers")
def view(repo: str | None, start: int, count: int, numbers: bool):
    """View lines from the current state of the log."""
    repo_path = get_repo_path(repo)
    log_repo = LogRepo.open(repo_path)
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
@click.option("--repo", "-r", default=None, help="Repository path")
def filter(pattern: str, keep: bool, repo: str | None):
    """Filter lines by regex pattern.

    PATTERN is a regular expression. Use --keep (default) to keep matching
    lines, or --remove to remove them.
    """
    repo_path = get_repo_path(repo)
    log_repo = LogRepo.open(repo_path)

    before = log_repo.current_line_count()
    log_repo.filter(pattern, keep)
    after = log_repo.current_line_count()

    action = "kept" if keep else "removed"
    diff = abs(after - before)
    console.print(f"[green]Filter applied:[/green] {action} {diff:,} lines (/{pattern}/)")
    console.print(f"  Lines: {before:,} → {after:,}")


@main.command()
@click.argument("pattern")
@click.argument("replacement")
@click.option("--repo", "-r", default=None, help="Repository path")
def replace(pattern: str, replacement: str, repo: str | None):
    """Replace text matching a regex pattern.

    PATTERN is a regular expression. REPLACEMENT can include capture
    group references like $1, $2, etc.
    """
    repo_path = get_repo_path(repo)
    log_repo = LogRepo.open(repo_path)

    log_repo.replace(pattern, replacement)
    console.print(f"[green]Replace applied:[/green] /{pattern}/ → \"{replacement}\"")


@main.command()
@click.argument("indices", nargs=-1, type=int, required=True)
@click.option("--repo", "-r", default=None, help="Repository path")
def delete(indices: tuple[int, ...], repo: str | None):
    """Delete specific lines by their indices (0-based)."""
    repo_path = get_repo_path(repo)
    log_repo = LogRepo.open(repo_path)

    log_repo.delete_lines(list(indices))
    console.print(f"[green]Deleted {len(indices)} line(s)[/green]")


@main.command()
@click.argument("after_line", type=int)
@click.argument("content", nargs=-1, required=True)
@click.option("--repo", "-r", default=None, help="Repository path")
def insert(after_line: int, content: tuple[str, ...], repo: str | None):
    """Insert lines after the specified position.

    AFTER_LINE is the position to insert after (0 = insert at beginning).
    """
    repo_path = get_repo_path(repo)
    log_repo = LogRepo.open(repo_path)

    log_repo.insert_lines(after_line, list(content))
    console.print(f"[green]Inserted {len(content)} line(s) after line {after_line}[/green]")


@main.command()
@click.argument("line_index", type=int)
@click.argument("new_content")
@click.option("--repo", "-r", default=None, help="Repository path")
def modify(line_index: int, new_content: str, repo: str | None):
    """Modify a single line by its index (0-based)."""
    repo_path = get_repo_path(repo)
    log_repo = LogRepo.open(repo_path)

    log_repo.modify_line(line_index, new_content)
    console.print(f"[green]Modified line {line_index}[/green]")


@main.command()
@click.option("--repo", "-r", default=None, help="Repository path")
def undo(repo: str | None):
    """Undo the last operation."""
    repo_path = get_repo_path(repo)
    log_repo = LogRepo.open(repo_path)

    desc = log_repo.undo()
    console.print(f"[green]Undone:[/green] {desc}")


@main.command()
@click.option("--repo", "-r", default=None, help="Repository path")
def history(repo: str | None):
    """Show operation history."""
    repo_path = get_repo_path(repo)
    log_repo = LogRepo.open(repo_path)

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
@click.option("--repo", "-r", default=None, help="Repository path")
def export(dest: str, repo: str | None):
    """Export current state to a text file."""
    repo_path = get_repo_path(repo)
    log_repo = LogRepo.open(repo_path)

    with console.status("Exporting..."):
        log_repo.export(dest)

    console.print(f"[green]Exported to:[/green] {dest}")


@main.command(name="clone")
@click.argument("dest", type=click.Path())
@click.option("--repo", "-r", default=None, help="Source repository path")
def clone_cmd(dest: str, repo: str | None):
    """Clone a repository to a new location."""
    repo_path = get_repo_path(repo)
    log_repo = LogRepo.open(repo_path)

    with console.status("Cloning repository..."):
        log_repo.clone_to(dest)

    console.print(f"[green]Cloned to:[/green] {dest}")


@main.command()
@click.argument("pattern")
@click.option("--repo", "-r", default=None, help="Repository path")
@click.option("--count", "-n", default=20, help="Max results to show")
def search(pattern: str, repo: str | None, count: int):
    """Search for lines matching a regex pattern (read-only, no modification)."""
    repo_path = get_repo_path(repo)
    log_repo = LogRepo.open(repo_path)

    import re
    compiled = re.compile(pattern)
    lines = log_repo.read_all_lines()
    matches = 0

    for i, line in enumerate(lines):
        if compiled.search(line):
            console.print(f"[dim]{i:>8}[/dim] {line}")
            matches += 1
            if matches >= count:
                console.print(f"\n[dim]... showing first {count} matches. Use -n to show more.[/dim]")
                break

    if matches == 0:
        console.print(f"[dim]No matches found for /{pattern}/[/dim]")
    else:
        console.print(f"\n[green]{matches} match(es) shown[/green]")


def _format_size(size_bytes: int) -> str:
    """Format byte size to human-readable string."""
    for unit in ["B", "KB", "MB", "GB", "TB"]:
        if size_bytes < 1024:
            return f"{size_bytes:.1f} {unit}"
        size_bytes /= 1024
    return f"{size_bytes:.1f} PB"


if __name__ == "__main__":
    main()
