#!/usr/bin/env python3
"""
Performance benchmark: log-analyzer vs common text processing tools.

Compares against grep, ripgrep (rg), awk, and Python stdlib on realistic
tasks using the same log file. Measures wall-clock time for each.

Usage:
    python3 benchmarks/bench.py [LOG_FILE]

Default LOG_FILE: log/log.json
"""

import os
import shutil
import subprocess
import sys
import tempfile
import time
from dataclasses import dataclass, field

# ---------------------------------------------------------------------------
# Configuration
# ---------------------------------------------------------------------------

SCRIPT_DIR = os.path.dirname(os.path.abspath(__file__))
PROJECT_DIR = os.path.dirname(SCRIPT_DIR)
DEFAULT_LOG = os.path.join(PROJECT_DIR, "log", "log.json")

LOG_FILE = sys.argv[1] if len(sys.argv) > 1 else DEFAULT_LOG
if not os.path.isfile(LOG_FILE):
    print(f"Log file not found: {LOG_FILE}")
    sys.exit(1)

FILE_SIZE_MB = os.path.getsize(LOG_FILE) / 1024 / 1024
REPO_PATH = os.path.join(tempfile.mkdtemp(), "bench_repo")

# Check available tools
HAVE_RG = shutil.which("rg") is not None
HAVE_AWK = shutil.which("awk") is not None


# ---------------------------------------------------------------------------
# Helpers
# ---------------------------------------------------------------------------

@dataclass
class Result:
    tool: str
    task: str
    time_s: float
    output: str = ""


def run_shell(cmd: str) -> tuple[float, str]:
    """Run a shell command, return (elapsed_seconds, stdout_last_line)."""
    start = time.perf_counter()
    proc = subprocess.run(cmd, shell=True, capture_output=True, text=True)
    elapsed = time.perf_counter() - start
    out = proc.stdout.strip().split("\n")[-1] if proc.stdout.strip() else ""
    return elapsed, out


def run_python(fn) -> tuple[float, str]:
    """Run a Python callable, return (elapsed_seconds, result_str)."""
    start = time.perf_counter()
    result = fn()
    elapsed = time.perf_counter() - start
    return elapsed, str(result)


def fmt(seconds: float) -> str:
    if seconds < 1:
        return f"{seconds * 1000:7.0f} ms"
    return f"{seconds:7.2f} s "


def separator():
    print("-" * 78)


# ---------------------------------------------------------------------------
# Import / prepare repo once
# ---------------------------------------------------------------------------

print(f"Log file:  {LOG_FILE} ({FILE_SIZE_MB:.0f} MB)")
print(f"Repo path: {REPO_PATH}")
print()

print("Importing into log-analyzer repo (one-time cost)...")
t0 = time.perf_counter()
from log_analyzer import LogRepo
repo = LogRepo.import_file(REPO_PATH, LOG_FILE)
import_time = time.perf_counter() - t0
meta = repo.metadata()
print(f"  {meta.original_line_count:,} lines imported in {fmt(import_time)}")
print()

results: list[Result] = []


# ---------------------------------------------------------------------------
# Task 1: Count lines matching "sending"
# ---------------------------------------------------------------------------

PATTERN = "sending"
TASK = f"count \"{PATTERN}\""
print(f"=== Task 1: {TASK} ===")

# grep -c
t, out = run_shell(f'grep -c "{PATTERN}" "{LOG_FILE}"')
results.append(Result("grep", TASK, t, out))
print(f"  grep -c:          {fmt(t)}  result={out}")

# ripgrep
if HAVE_RG:
    t, out = run_shell(f'rg -c "{PATTERN}" "{LOG_FILE}"')
    results.append(Result("ripgrep", TASK, t, out))
    print(f"  rg -c:            {fmt(t)}  result={out}")

# awk
if HAVE_AWK:
    t, out = run_shell(f'awk \'/{PATTERN}/{{n++}} END{{print n}}\' "{LOG_FILE}"')
    results.append(Result("awk", TASK, t, out))
    print(f"  awk:              {fmt(t)}  result={out}")

# wc + grep (pipe)
t, out = run_shell(f'grep "{PATTERN}" "{LOG_FILE}" | wc -l')
results.append(Result("grep|wc", TASK, t, out))
print(f"  grep|wc -l:       {fmt(t)}  result={out}")

# Python stdlib
def py_count():
    n = 0
    with open(LOG_FILE) as f:
        for line in f:
            if PATTERN in line:
                n += 1
    return n

t, out = run_python(py_count)
results.append(Result("python", TASK, t, out))
print(f"  python:           {fmt(t)}  result={out}")

# log-analyzer (collector on repo, already imported)
t, out = run_python(lambda: repo.collect_count(PATTERN))
results.append(Result("log-analyzer", TASK, t, out))
print(f"  log-analyzer:     {fmt(t)}  result={out}")

# log-analyzer stream count
t, out = run_python(lambda: repo.count_matches(PATTERN))
results.append(Result("log-analyzer(stream)", TASK, t, out))
print(f"  log-analyzer(st): {fmt(t)}  result={out}")

separator()

# ---------------------------------------------------------------------------
# Task 2: Filter matching lines to a file
# ---------------------------------------------------------------------------

TASK2 = f"filter \"{PATTERN}\" to file"
print(f"\n=== Task 2: {TASK2} ===")

with tempfile.TemporaryDirectory() as tmpdir:
    out_grep = os.path.join(tmpdir, "grep.out")
    out_rg   = os.path.join(tmpdir, "rg.out")
    out_awk  = os.path.join(tmpdir, "awk.out")
    out_py   = os.path.join(tmpdir, "py.out")
    out_la   = os.path.join(tmpdir, "la.out")

    t, _ = run_shell(f'grep "{PATTERN}" "{LOG_FILE}" > "{out_grep}"')
    results.append(Result("grep", TASK2, t))
    print(f"  grep > file:      {fmt(t)}")

    if HAVE_RG:
        t, _ = run_shell(f'rg "{PATTERN}" "{LOG_FILE}" > "{out_rg}"')
        results.append(Result("ripgrep", TASK2, t))
        print(f"  rg > file:        {fmt(t)}")

    if HAVE_AWK:
        t, _ = run_shell(f'awk \'/{PATTERN}/\' "{LOG_FILE}" > "{out_awk}"')
        results.append(Result("awk", TASK2, t))
        print(f"  awk > file:       {fmt(t)}")

    def py_filter():
        with open(LOG_FILE) as fin, open(out_py, "w") as fout:
            for line in fin:
                if PATTERN in line:
                    fout.write(line)
        return 0

    t, _ = run_python(py_filter)
    results.append(Result("python", TASK2, t))
    print(f"  python:           {fmt(t)}")

    t, out = run_python(lambda: repo.stream_filter_to_file(PATTERN, True, out_la))
    results.append(Result("log-analyzer", TASK2, t, out))
    print(f"  log-analyzer:     {fmt(t)}  lines={out}")

separator()

# ---------------------------------------------------------------------------
# Task 3: Regex replace and write to file
# ---------------------------------------------------------------------------

REPLACE_PAT = r"\d{1,3}\.\d{1,3}\.\d{1,3}\.\d{1,3}"
REPLACE_REP = "X.X.X.X"
TASK3 = "regex replace IPs"
print(f"\n=== Task 3: {TASK3} ===")

with tempfile.TemporaryDirectory() as tmpdir:
    out_sed = os.path.join(tmpdir, "sed.out")
    out_py  = os.path.join(tmpdir, "py.out")
    out_la  = os.path.join(tmpdir, "la.out")

    t, _ = run_shell(
        f"sed -E 's/{REPLACE_PAT}/{REPLACE_REP}/g' \"{LOG_FILE}\" > \"{out_sed}\""
    )
    results.append(Result("sed", TASK3, t))
    print(f"  sed > file:       {fmt(t)}")

    if HAVE_RG:
        t, _ = run_shell(
            f'rg "{REPLACE_PAT}" "{LOG_FILE}" -r "{REPLACE_REP}" > "{os.path.join(tmpdir, "rg.out")}"'
        )
        results.append(Result("ripgrep", TASK3, t))
        print(f"  rg -r > file:     {fmt(t)}")

    import re as _re
    _compiled = _re.compile(REPLACE_PAT)

    def py_replace():
        with open(LOG_FILE) as fin, open(out_py, "w") as fout:
            for line in fin:
                fout.write(_compiled.sub(REPLACE_REP, line))
        return 0

    t, _ = run_python(py_replace)
    results.append(Result("python", TASK3, t))
    print(f"  python re.sub:    {fmt(t)}")

    t, out = run_python(
        lambda: repo.stream_replace_to_file(REPLACE_PAT, REPLACE_REP, out_la)
    )
    results.append(Result("log-analyzer", TASK3, t, out))
    print(f"  log-analyzer:     {fmt(t)}  modified={out}")

separator()

# ---------------------------------------------------------------------------
# Task 4: Group-count (unique values of a field)
# ---------------------------------------------------------------------------

TASK4 = 'group count by __SOURCE__'
print(f"\n=== Task 4: {TASK4} ===")

if HAVE_AWK:
    t, out = run_shell(
        f"""rg -oP '"__SOURCE__":"([^"]+)"' -r '$1' "{LOG_FILE}" | sort | uniq -c | sort -rn"""
    )
    results.append(Result("rg|sort|uniq", TASK4, t))
    print(f"  rg|sort|uniq -c:  {fmt(t)}")

def py_group():
    from collections import Counter
    pat = _re.compile(r'"__SOURCE__":"([^"]+)"')
    c = Counter()
    with open(LOG_FILE) as f:
        for line in f:
            m = pat.search(line)
            if m:
                c[m.group(1)] += 1
    return dict(c)

t, out = run_python(py_group)
results.append(Result("python", TASK4, t))
print(f"  python Counter:   {fmt(t)}")

t, out = run_python(lambda: repo.collect_group_count(r'"__SOURCE__":"([^"]+)"', 1))
results.append(Result("log-analyzer", TASK4, t))
print(f"  log-analyzer:     {fmt(t)}")

separator()

# ---------------------------------------------------------------------------
# Summary table
# ---------------------------------------------------------------------------

print("\n\n========== SUMMARY ==========\n")
print(f"File: {LOG_FILE} ({FILE_SIZE_MB:.0f} MB, {meta.original_line_count:,} lines)")
print(f"Import time: {fmt(import_time)}")
print()

tasks = sorted(set(r.task for r in results), key=lambda t: [r.task for r in results].index(t))
tools_order = ["grep", "grep|wc", "ripgrep", "awk", "sed", "rg|sort|uniq", "python",
               "log-analyzer", "log-analyzer(stream)"]

for task in tasks:
    print(f"  {task}")
    task_results = [r for r in results if r.task == task]
    task_results.sort(key=lambda r: r.time_s)
    fastest = task_results[0].time_s if task_results else 1
    for r in task_results:
        ratio = r.time_s / fastest if fastest > 0 else 0
        bar = "#" * min(int(ratio * 10), 50)
        print(f"    {r.tool:<22s} {fmt(r.time_s)}  {ratio:5.1f}x  {bar}")
    print()

# ---------------------------------------------------------------------------
# Usability comparison
# ---------------------------------------------------------------------------

print("========== USABILITY COMPARISON ==========\n")
print("""\
| Feature                  | grep/rg/sed/awk        | log-analyzer            |
|--------------------------|------------------------|-------------------------|
| Count matches            | grep -c / rg -c        | collect_count(pattern)  |
| Filter to file           | grep pattern > out      | stream_filter_to_file() |
| Regex replace            | sed -E 's/.../.../'     | stream_replace_to_file()|
| Group-by counting        | rg|sort|uniq -c (pipe)  | collect_group_count()   |
| Top-N frequency          | ...| sort | head -N     | collect_top_n()         |
| Numeric stats            | awk (manual script)     | collect_numeric_stats() |
| Undo last operation      | not possible            | undo()                  |
| Operation history        | not possible            | history()               |
| Compressed storage       | no (raw files)          | zstd chunks             |
| Append / concat files    | cat >> file             | append_file()           |
| Branching analysis       | cp -r + manual          | clone_to()              |
| Random line access       | sed -n 'Np' (slow)      | read_line(N) (indexed)  |
| Python API               | subprocess only         | native import           |
""")

# cleanup
shutil.rmtree(os.path.dirname(REPO_PATH), ignore_errors=True)

print("Done.")
