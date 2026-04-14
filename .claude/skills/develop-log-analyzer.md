---
description: Develop and extend the log-analyzer project. Build, test, add operators, modify the Rust core or Python CLI. Covers the full dev workflow.
---

# Develop Log Analyzer

## Build & Test

```bash
# Install in development mode (builds Rust + installs Python)
pip install -e ".[dev]"

# Run Rust tests (unit + integration)
cargo test

# Run Python tests
pytest tests/ -v

# Run all tests
cargo test && pytest tests/ -v
```

## Adding a New Operator

1. Create `src/operator/<name>.rs` with `apply()` and `apply_with_inverse()` methods.
2. Add variant to `Operation` enum in `src/operator/mod.rs`.
3. Wire up `apply()` and `apply_with_inverse()` match arms.
4. Add PyO3 binding method in `src/bindings.rs`.
5. Add CLI command in `python/log_analyzer/cli.py`.
6. Add tests in both `tests/test_repo.rs` and `tests/test_python_repo.py`.

## Key Design Principles

- **All operators must be reversible**: Store inverse data for undo.
- **Performance**: Use `rayon` for parallel processing when line count > 10,000.
- **Compression**: All stored data uses zstd compression.
- **Chunked storage**: Lines are grouped into chunks of 10,000 for efficient random access.

## File Guide

| File | Purpose |
|------|---------|
| `src/lib.rs` | PyO3 module entry point |
| `src/bindings.rs` | Python class/method bindings |
| `src/error.rs` | Error types with PyErr conversion |
| `src/repo/mod.rs` | LogRepo struct - main API |
| `src/repo/storage.rs` | ChunkStorage - zstd compressed chunks |
| `src/repo/metadata.rs` | RepoMetadata - UUID, timestamps, stats |
| `src/index/mod.rs` | LineIndex - chunk-based line lookup |
| `src/index/builder.rs` | IndexBuilder - parallel line scanning |
| `src/operator/mod.rs` | Operation enum, InverseData, dispatch |
| `src/operator/filter.rs` | Regex filter (keep/remove) |
| `src/operator/replace.rs` | Regex replace with capture groups |
| `src/operator/crud.rs` | DeleteLines, InsertLines, ModifyLine |
| `python/log_analyzer/cli.py` | Click CLI commands |
