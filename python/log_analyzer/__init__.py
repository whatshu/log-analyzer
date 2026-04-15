"""
log-analyzer: High-performance log analyzer for large text files.

Uses a Rust backend for compressed storage and multi-threaded processing,
with a Python CLI frontend.
"""

from log_analyzer._core import LogRepo, RepoMetadata, OperationRecord, Workspace

__all__ = ["LogRepo", "RepoMetadata", "OperationRecord", "Workspace"]
__version__ = "0.1.0"
