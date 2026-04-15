# log-analyzer

[中文文档](README_zh.md)

A high-performance log analysis tool built with a **Rust** backend and **Python** CLI frontend. Designed for text log files exceeding **10 GB**, it stores logs in compressed repositories with a full operation history and undo support.

## Features

- **Compressed storage** — Logs are split into chunks and compressed with zstd. A 900 MB file typically compresses to ~100 MB on disk.
- **Reversible operations** — Every operation (filter, replace, delete, insert, modify) records inverse data, enabling unlimited undo.
- **Git-like repositories** — Each repository maintains an operation journal. Repositories can be cloned for parallel analysis branches.
- **Streaming engine** — Filter, replace, search, and collect statistics chunk-by-chunk without loading the entire file into memory.
- **Collectors** — Read-only terminal operations (inspired by Java Stream Collectors) for aggregation: count, group-by, top-N, unique values, numeric statistics.
- **Multi-threaded** — Parallel chunk processing via [rayon](https://github.com/rayon-rs/rayon) for indexing, filtering, searching, and collecting.
- **Append** — Incrementally add new log data to an existing repository without re-importing.
- **Python API** — Full Rust functionality exposed to Python via [PyO3](https://pyo3.rs), usable both as a library and through the CLI.

## Installation

Requires Python >= 3.10 and Rust toolchain (for building the native extension).

```bash
pip install -e ".[dev]"
```

This uses [maturin](https://github.com/PyO3/maturin) to compile the Rust core and install the `log-analyzer` CLI.

## Quick Start

```bash
# Import a log file
log-analyzer import server.log

# View the first 20 lines
log-analyzer view

# Filter to keep only ERROR lines
log-analyzer filter "ERROR" --keep

# See what happened
log-analyzer history

# Undo the filter
log-analyzer undo

# Export the current state
log-analyzer export filtered.log
```

## CLI Reference

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
| `clone <dest>` | Clone the repository for parallel analysis |

All commands accept `--repo <path>` (default: `.logrepo/`).

## Python API

```python
from log_analyzer import LogRepo

# Import
repo = LogRepo.import_file("./repo", "server.log")

# Or open existing
repo = LogRepo.open("./repo")

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

# Export and clone
repo.export("output.log")
cloned = repo.clone_to("./repo_copy")
```

## Examples

### Analyzing a 900 MB JSON log

```python
from log_analyzer import LogRepo

repo = LogRepo.import_file(".logrepo", "log/log.json")

# Count "sending" messages
sending = repo.collect_count("sending")
total = repo.metadata().original_line_count
print(f"sending: {sending:,} / {total:,} ({sending/total*100:.1f}%)")

# Break down by target
targets = repo.collect_group_count(r"sending to legacy (\w+)", 1)
for target, count in sorted(targets.items(), key=lambda x: -x[1]):
    print(f"  -> {target}: {count:,}")

# Top 10 clients
for client_id, count in repo.collect_top_n(r"clientId=(\d+)", 1, 10):
    print(f"  clientId={client_id}: {count:,}")

# Payload size statistics
stats = repo.collect_numeric_stats(r"payloadLen=(\d+)", 1)
print(f"payloadLen: min={stats['min']:.0f} max={stats['max']:.0f} avg={stats['avg']:.1f}")
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
# Create a base repo
log-analyzer import access.log --repo base

# Clone for two independent analyses
log-analyzer clone error_analysis --repo base
log-analyzer clone perf_analysis  --repo base

# Analyze errors
log-analyzer filter '" 500 ' --keep --repo error_analysis
log-analyzer export 500_errors.log  --repo error_analysis

# Analyze performance
log-analyzer filter 'slow\|timeout' --keep --repo perf_analysis
```

### Anonymizing sensitive data

```bash
log-analyzer import access.log
log-analyzer replace '\d{1,3}\.\d{1,3}\.\d{1,3}\.\d{1,3}' 'X.X.X.X'
log-analyzer replace 'user=\w+' 'user=REDACTED'
log-analyzer export anonymized.log
```

## Architecture

```
src/                        Rust core (compiled to Python extension via PyO3)
├── lib.rs                  PyO3 module entry
├── bindings.rs             Python class/method bindings
├── error.rs                Error types
├── repo/                   Log repository
│   ├── mod.rs              LogRepo: create, open, append, operations, undo
│   ├── storage.rs          ChunkStorage: zstd compressed chunk I/O
│   └── metadata.rs         RepoMetadata: UUID, timestamps, stats
├── index/                  Line indexing
│   ├── mod.rs              LineIndex: chunk-based line lookup
│   └── builder.rs          IndexBuilder: parallel newline scanning
├── operator/               Reversible operators
│   ├── mod.rs              Operation enum, InverseData, dispatch
│   ├── filter.rs           Regex filter (keep/remove)
│   ├── replace.rs          Regex replace with capture groups
│   └── crud.rs             DeleteLines, InsertLines, ModifyLine
└── engine/                 Streaming processing engine
    ├── mod.rs              Shared chunk reading utilities
    ├── stream.rs           LineStream: chunk-by-chunk iterator
    ├── processor.rs        ChunkedProcessor: streaming filter/replace/search
    └── collector.rs        Collector: count, group_count, top_n, unique, numeric_stats

python/log_analyzer/        Python package
├── __init__.py             Re-exports LogRepo, RepoMetadata, OperationRecord
└── cli.py                  Click CLI (import, append, view, filter, replace, ...)

tests/                      Test suite (61 Rust + 85 Python = 146 tests)
```

### Repository layout on disk

```
.logrepo/
├── meta.json               Repository metadata (ID, source, size, line count)
├── index.json              Line index (chunk boundaries, byte offsets)
├── chunks/                 Compressed data chunks
│   ├── 000000.zst
│   ├── 000001.zst
│   └── ...
└── operations.json         Operation journal (for undo/redo)
```

## Testing

```bash
# Rust tests (unit + integration)
cargo test

# Python tests (unit + integration + scenario)
pytest tests/ -v

# All tests
cargo test && pytest tests/ -v
```

## License

MIT
