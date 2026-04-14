# Log Analyzer

A high-performance log analysis tool with a Rust backend and Python CLI frontend. Designed for analyzing text log files >10GB.

## Architecture

- **Rust core** (`src/`): Storage engine with zstd compression, line indexing, operators (filter/replace/CRUD), multi-threaded via rayon, exposed to Python via PyO3.
- **Python frontend** (`python/log_analyzer/`): Click-based CLI with rich output.
- **Build**: maturin (pyproject.toml + Cargo.toml).

## Key Concepts

- **Log Repository (日志仓)**: Stores compressed log data in chunks with a line index. Git-like operation journal for undo/redo. Located in a directory (default `.logrepo/`).
- **Operators (操作符)**: Reversible transformations on log lines: filter (regex), replace (regex), delete, insert, modify. All operations record inverse data for undo.

## Development

```bash
pip install -e ".[dev]"       # Build & install (includes maturin)
cargo test                     # Rust tests (35 tests)
pytest tests/                  # Python tests (41 tests)
log-analyzer --help            # CLI usage
```

## Project Layout

```
src/                     # Rust source
├── lib.rs               # PyO3 module entry
├── bindings.rs          # Python bindings
├── error.rs             # Error types
├── repo/                # Repository: storage, metadata, chunk management
├── operator/            # Operators: filter, replace, CRUD
└── index/               # Line indexing and chunk building
python/log_analyzer/     # Python package
├── __init__.py          # Re-exports from _core
└── cli.py               # Click CLI commands
tests/                   # Rust integration + Python tests
```
