# log-analyzer

[中文文档](README_zh.md)

A high-performance log analysis tool built with a **Rust** backend and **Python** CLI frontend. Designed for text log files exceeding **10 GB**, it stores logs in compressed repositories with a full operation history and undo support. Uses [ripgrep](https://github.com/BurntSushi/ripgrep)'s SIMD-accelerated search engine internally.

## Features

- **Compressed storage** — Logs are split into chunks and compressed with zstd. A 900 MB file typically compresses to ~100 MB on disk.
- **Reversible operations** — Every operation (filter, replace, delete, insert, modify) records inverse data, enabling unlimited undo.
- **Git-like repositories** — Each repository maintains an operation journal. Repositories can be cloned for parallel analysis branches.
- **Streaming engine** — Filter, replace, search, and collect statistics chunk-by-chunk without loading the entire file into memory.
- **Collectors** — Read-only terminal operations (inspired by Java Stream Collectors) for aggregation: count, group-by, top-N, unique values, numeric statistics.
- **ripgrep-powered search** — Pattern matching uses [grep-searcher](https://crates.io/crates/grep-searcher) with SIMD literal optimizations from the ripgrep ecosystem.
- **Multi-threaded** — Parallel chunk processing via [rayon](https://github.com/rayon-rs/rayon) for indexing, filtering, searching, and collecting.
- **Append** — Incrementally add new log data to an existing repository without re-importing.
- **Python API** — Full Rust functionality exposed to Python via [PyO3](https://pyo3.rs), usable both as a library and through the CLI.

## Building

Requires **Python >= 3.10** and **Rust toolchain** (rustc + cargo).

A `build.sh` script is provided for all build tasks:

```bash
./build.sh                    # Build release wheel only (no install)
./build.sh --dev              # Build debug wheel only
./build.sh install            # Build release wheel and install
./build.sh install --dev      # Editable development install (rebuilds on Rust changes)
./build.sh uninstall          # Remove installed package
./build.sh test               # Install and run full test suite
```

Or manually:

```bash
pip install -e ".[dev]"       # Editable install (uses maturin)
maturin build --release       # Build .whl to target/wheels/
```

## Quick Start

```bash
# Import a log file (creates "default" repo)
log-analyzer import server.log

# View the first 20 lines
log-analyzer view

# Filter to keep only ERROR lines
log-analyzer filter "ERROR" --keep

# Undo the filter
log-analyzer undo

# Clone for a separate analysis branch
log-analyzer repo clone default errors
log-analyzer repo use errors
log-analyzer filter "ERROR" --keep

# Switch back — original is untouched
log-analyzer repo use default
log-analyzer view

# List all repos
log-analyzer repo list

# Export
log-analyzer export filtered.log
```

## CLI Reference

### Log operations

| Command | Description |
|---------|-------------|
| `import <file>` | Import a text file into a new repository |
| `append <file>` | Append a text file into an existing repository |
| `info` | Show repository metadata and operation count |
| `view` | View lines from the current state |
| `search <pattern>` | Search for regex matches (read-only) |
| `filter <pattern>` | Keep (`--keep`) or remove (`--remove`) matching lines |
| `replace <pattern> <replacement>` | Regex replace (supports `$1`, `$2` capture groups) |
| `delete <indices...>` | Delete lines by 0-based index |
| `insert <after> <content...>` | Insert lines after a position |
| `modify <index> <content>` | Replace a single line |
| `undo` | Undo the last operation |
| `history` | Show the operation journal |
| `export <file>` | Write the current state to a file |

### Repository management

| Command | Description |
|---------|-------------|
| `repo list` | List all repos in the workspace (`*` marks active) |
| `repo use <name>` | Switch the active repository |
| `repo clone <src> <dst>` | Clone a repo under a new name |
| `repo remove <name>` | Delete a repository |

All log commands accept `--repo <name>` to target a specific repo (default: active repo).
The workspace directory defaults to `.logrepo/` and can be changed with `--workspace <path>`.

## Python API

```python
from log_analyzer import Workspace

# Open workspace (auto-creates on first import)
ws = Workspace(".logrepo")

# Import into a named repo
ws.import_file("server.log", "default")

# Manage repos
ws.clone_repo("default", "errors")
ws.set_active("errors")
repo = ws.open_active()          # or ws.open_repo("errors")
print(ws.list())                  # ["default", "errors"]

# Low-level: open a repo directly by path
from log_analyzer import LogRepo
repo = LogRepo.open("./some/path")

# Read lines
lines = repo.read_lines(0, 10)       # first 10 lines
line  = repo.read_line(42)           # single line

# Operations (all reversible)
repo.filter(r"\[ERROR\]", keep=True)
repo.replace(r"\d{4}-\d{2}-\d{2}", "DATE")
repo.delete_lines([0, 5, 10])
repo.insert_lines(0, ["# header"])
repo.modify_line(3, "new content")

# Undo
repo.undo()

# Append new data
repo.append_file("server_day2.log")
repo.append_text("extra line\n")

# Collectors (read-only, does not modify the repo)
repo.collect_count("ERROR")                          # -> 4821
repo.collect_group_count(r"\[(\w+)\]", 1)            # -> {"ERROR": 4821, "INFO": 30102, ...}
repo.collect_top_n(r"clientId=(\d+)", 1, 5)          # -> [("1234", 500), ...]
repo.collect_unique(r"src=(\S+)", 1)                  # -> ["10.0.0.1", "10.0.0.2"]
repo.collect_numeric_stats(r"latency=(\d+)ms", 1)    # -> {"count": ..., "min": ..., "max": ..., "avg": ..., "sum": ...}
repo.collect_line_stats()                              # -> {"count": ..., "avg_len": ..., ...}

# Streaming (memory-efficient for large files)
repo.stream_search("ERROR", max_results=100)          # -> [(line_num, content), ...]
repo.stream_filter_to_file("ERROR", True, "err.log")  # write matches to file
repo.count_matches("ERROR")                            # count without loading all data

# Search files directly without importing (ripgrep-powered)
LogRepo.count_file_matches("server.log", "ERROR")     # -> 4821
LogRepo.search_file("server.log", "ERROR", 10)        # -> [(line_num, content), ...]

# Export and clone
repo.export("output.log")
cloned = repo.clone_to("./repo_copy")
```

## Examples

### Analyzing a large JSON log

```python
from log_analyzer import LogRepo

repo = LogRepo.import_file(".logrepo", "access.log")

# Count specific messages
errors = repo.collect_count("ERROR")
total = repo.metadata().original_line_count
print(f"errors: {errors:,} / {total:,} ({errors/total*100:.1f}%)")

# Group by log level
levels = repo.collect_group_count(r"\[(\w+)\]", 1)
for level, count in sorted(levels.items(), key=lambda x: -x[1]):
    print(f"  {level}: {count:,}")

# Top 10 clients
for client_id, count in repo.collect_top_n(r"clientId=(\d+)", 1, 10):
    print(f"  clientId={client_id}: {count:,}")

# Response time statistics
stats = repo.collect_numeric_stats(r"latency=(\d+)ms", 1)
print(f"latency: min={stats['min']:.0f} max={stats['max']:.0f} avg={stats['avg']:.1f}")
```

### Concatenating split log files

```bash
log-analyzer import logs/part1.log
log-analyzer append logs/part2.log
log-analyzer append logs/part3.log
log-analyzer info   # shows total lines across all parts
```

### Branching analysis

```bash
# Import base data
log-analyzer import access.log

# Clone for two independent analyses
log-analyzer repo clone default error_analysis
log-analyzer repo clone default perf_analysis

# Analyze errors
log-analyzer repo use error_analysis
log-analyzer filter '" 500 ' --keep
log-analyzer export 500_errors.log

# Analyze performance (target by name without switching)
log-analyzer filter 'slow\|timeout' --keep --repo perf_analysis

# Original data untouched
log-analyzer view --repo default
```

### Anonymizing sensitive data

```bash
log-analyzer import access.log
log-analyzer replace '\d{1,3}\.\d{1,3}\.\d{1,3}\.\d{1,3}' 'X.X.X.X'
log-analyzer replace 'user=\w+' 'user=REDACTED'
log-analyzer export anonymized.log
```

## Benchmarks

Tested on a 897 MB log file (2.15 million lines). One-time import: **~550 ms**.

Run the benchmark yourself: `python3 benchmarks/bench.py [LOG_FILE]`

### Performance comparison

| Task | grep | ripgrep | sed | awk | Python | log-analyzer |
|------|------|---------|-----|-----|--------|--------------|
| Count matches | 200 ms | **116 ms** | — | 701 ms | 327 ms | 178 ms |
| Filter to file | 275 ms | **181 ms** | — | 750 ms | 405 ms | 557 ms |
| Regex replace | — | 948 ms | **640 ms** | — | 7.17 s | 967 ms |
| Group-by count | — | 1.26 s* | — | — | 938 ms | **250 ms** |

*\* rg \| sort \| uniq -c pipeline*

**Key takeaways:**

- **Counting/searching**: log-analyzer is 1.5x ripgrep (uses ripgrep's grep-searcher internally), faster than grep, awk, and Python.
- **Regex replace**: On par with sed and ripgrep; 7x faster than Python.
- **Aggregation (group-by, top-N, stats)**: **log-analyzer is fastest** — 3.7x faster than Python, 5x faster than rg|sort|uniq pipe. This is the core advantage over Unix tools.
- **Filter to file**: ripgrep and grep are faster for raw file-to-file filtering since they avoid decompression overhead.

### Usability comparison

| Feature | grep/rg/sed/awk | log-analyzer |
|---------|-----------------|--------------|
| Count matches | `grep -c` / `rg -c` | `collect_count(pattern)` |
| Filter to file | `grep pattern > out` | `stream_filter_to_file()` |
| Regex replace | `sed -E 's/.../.../'` | `stream_replace_to_file()` |
| Group-by counting | `rg \| sort \| uniq -c` | `collect_group_count()` |
| Top-N frequency | `... \| sort \| head -N` | `collect_top_n()` |
| Numeric stats | awk (manual script) | `collect_numeric_stats()` |
| Undo last operation | not possible | `undo()` |
| Operation history | not possible | `history()` |
| Compressed storage | no (raw files) | zstd chunks |
| Append / concat files | `cat >> file` | `append_file()` |
| Branching analysis | `cp -r` + manual | `clone_to()` |
| Random line access | `sed -n 'Np'` (slow) | `read_line(N)` (indexed) |
| Python API | subprocess only | native import |

## Project Structure

```
log-analyzer/
├── build.sh                Build/install/uninstall script
├── Cargo.toml              Rust package config
├── pyproject.toml          Python package config (maturin)
│
├── src/                    Rust core (compiled to Python extension via PyO3)
│   ├── lib.rs              PyO3 module entry
│   ├── bindings.rs         Python class/method bindings
│   ├── error.rs            Error types
│   ├── repo/               Log repository
│   │   ├── mod.rs          LogRepo: create, open, append, operations, undo
│   │   ├── workspace.rs    Workspace: named repo management, clone, migrate
│   │   ├── storage.rs      ChunkStorage: zstd compressed chunk I/O
│   │   └── metadata.rs     RepoMetadata: UUID, timestamps, stats
│   ├── index/              Line indexing
│   │   ├── mod.rs          LineIndex: chunk-based line lookup
│   │   └── builder.rs      IndexBuilder: parallel newline scanning
│   ├── operator/           Reversible operators
│   │   ├── mod.rs          Operation enum, InverseData, dispatch
│   │   ├── filter.rs       Regex filter (keep/remove)
│   │   ├── replace.rs      Regex replace with capture groups
│   │   └── crud.rs         DeleteLines, InsertLines, ModifyLine
│   └── engine/             Streaming processing engine
│       ├── mod.rs          Shared chunk reading utilities
│       ├── fast.rs         ripgrep-powered SIMD search (grep-searcher)
│       ├── stream.rs       LineStream: chunk-by-chunk iterator
│       ├── processor.rs    ChunkedProcessor: streaming filter/replace/search
│       └── collector.rs    Collector: count, group_count, top_n, unique, numeric_stats
│
├── python/log_analyzer/    Python package
│   ├── __init__.py         Re-exports LogRepo, RepoMetadata, OperationRecord
│   └── cli.py              Click CLI (import, append, view, filter, replace, ...)
│
├── tests/                  Test suite (80 Rust + 101 Python = 181 tests)
├── benchmarks/             Performance benchmarks
│   └── bench.py            Comparison vs grep, rg, sed, awk, Python
└── .claude/                AI agent skills
```

### Workspace layout on disk

```
.logrepo/                       Workspace root
├── workspace.json              Active repo tracker: {"active": "default"}
├── default/                    Named repository
│   ├── meta.json               Repository metadata (ID, source, size, line count)
│   ├── index.json              Line index (chunk boundaries, byte offsets)
│   ├── chunks/                 Compressed data chunks
│   │   ├── 000000.zst
│   │   ├── 000001.zst
│   │   └── ...
│   └── operations.json         Operation journal (for undo/redo)
├── error_analysis/             Cloned repository (same structure)
│   └── ...
└── ...
```

Old flat `.logrepo/` layouts (pre-workspace) are auto-migrated on first open.

## Testing

```bash
cargo test                  # Rust tests (80 tests)
pytest tests/ -v            # Python tests (101 tests)
./build.sh test             # Build, install, and run all tests
```

## License

MIT
