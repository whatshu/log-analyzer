---
description: Analyze a log file using the log-analyzer tool. Import logs, filter by regex, replace patterns, search for errors, and manage operation history with undo support.
---

# Analyze Log File

Use the `log-analyzer` CLI tool to analyze log files. The tool stores logs in compressed repositories with full undo support.

## Available Commands

```bash
# Import a log file into a repository
log-analyzer import <file> [--repo <path>]

# View repository info
log-analyzer info [--repo <path>]

# View lines from current state
log-analyzer view [--start N] [--count N] [--repo <path>]

# Search for lines matching a regex (read-only)
log-analyzer search <pattern> [--count N] [--repo <path>]

# Filter lines by regex (keeps or removes matching lines)
log-analyzer filter <pattern> [--keep/--remove] [--repo <path>]

# Replace text using regex
log-analyzer replace <pattern> <replacement> [--repo <path>]

# CRUD operations on individual lines
log-analyzer delete <line_indices...> [--repo <path>]
log-analyzer insert <after_line> <content...> [--repo <path>]
log-analyzer modify <line_index> <new_content> [--repo <path>]

# Undo last operation
log-analyzer undo [--repo <path>]

# Show operation history
log-analyzer history [--repo <path>]

# Export filtered/modified log to file
log-analyzer export <dest> [--repo <path>]

# Clone a repository for parallel analysis
log-analyzer clone <dest> [--repo <path>]
```

## Common Workflows

### Error Investigation
```bash
log-analyzer import app.log
log-analyzer filter "ERROR" --keep
log-analyzer filter "database" --keep
log-analyzer view --count 50
```

### IP Anonymization
```bash
log-analyzer import access.log
log-analyzer replace '\d{1,3}\.\d{1,3}\.\d{1,3}\.\d{1,3}' 'X.X.X.X'
log-analyzer export anonymized.log
```

### Branching Analysis
```bash
log-analyzer import server.log --repo main
log-analyzer clone error_analysis --repo main
log-analyzer clone perf_analysis --repo main
log-analyzer filter "ERROR" --keep --repo error_analysis
log-analyzer filter "slow|timeout" --keep --repo perf_analysis
```

## Python API

```python
from log_analyzer import LogRepo

repo = LogRepo.import_file("./repo", "server.log")
repo.filter(r"\[ERROR\]", keep=True)
lines = repo.read_all_lines()
repo.undo()
repo.export("filtered.log")
```
