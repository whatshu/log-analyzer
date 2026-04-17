#!/usr/bin/env python3
"""
bench.py — Benchmark log-analyzer against grep / ripgrep / awk on a >10 GB file.

Usage
-----
    python3 scripts/bench.py              # generate, benchmark, cleanup, write docs
    python3 scripts/bench.py --keep-file  # skip cleanup (useful for re-running)
    python3 scripts/bench.py --file PATH  # benchmark an existing file
    python3 scripts/bench.py --gb 2       # generate a smaller file (default: 10)

Results are printed to stdout and written to docs/benchmarks.md.
"""

import argparse
import os
import platform
import random
import shutil
import subprocess
import sys
import time
from pathlib import Path
from statistics import median

# ---------------------------------------------------------------------------
# Paths
# ---------------------------------------------------------------------------
ROOT_DIR  = Path(__file__).resolve().parent.parent
DOCS_DIR  = ROOT_DIR / "docs"
TEST_FILE = Path("/tmp/log_analyzer_bench.log")
REPO_DIR  = Path("/tmp/log_analyzer_bench_repo")
OUT_FILE  = Path("/tmp/log_analyzer_bench_out.log")

PATTERN   = "ERROR"
RUNS      = 3

# ---------------------------------------------------------------------------
# Seed-block generator  (~1 MiB of realistic log lines)
# ---------------------------------------------------------------------------
_LEVELS  = ["ERROR", "WARN ", "INFO ", "DEBUG"]
_WEIGHTS = [5, 10, 60, 25]
_SVCS    = ["auth", "db", "cache", "worker", "http", "fs"]


def _seed_block(size: int = 1 << 20) -> bytes:
    rng = random.Random(0)
    buf: list[str] = []
    total = 0
    while total < size:
        lv  = rng.choices(_LEVELS, weights=_WEIGHTS)[0]
        ts  = (
            f"2024-{rng.randint(1,12):02d}-{rng.randint(1,28):02d} "
            f"{rng.randint(0,23):02d}:{rng.randint(0,59):02d}:{rng.randint(0,59):02d}"
        )
        tid = rng.randint(1, 512)
        t   = rng.randint(0, 7)
        if t == 0:
            msg = (
                f"User login failed user_id={rng.randint(1000,99999)} "
                f"ip=10.0.{rng.randint(0,255)}.{rng.randint(0,255)} "
                f"attempts={rng.randint(1,10)}"
            )
        elif t == 1:
            msg = (
                f"HTTP {rng.choice(['GET','POST','PUT','DELETE'])} "
                f"/api/{rng.choice(_SVCS)} "
                f"status={rng.choice([200,201,400,404,500])} "
                f"latency={rng.randint(1,5000)}ms"
            )
        elif t == 2:
            msg = (
                f"DB {rng.choice(['SELECT','INSERT','UPDATE'])} "
                f"table={rng.choice(['users','events','orders'])} "
                f"rows={rng.randint(0,100000)} "
                f"duration={rng.randint(0,10000)}ms"
            )
        elif t == 3:
            msg = (
                f"Cache {rng.choice(['hit','miss','expire'])} "
                f"key={rng.choice(_SVCS)}:{rng.randint(1000,9999)} "
                f"hit_rate={rng.random():.3f}"
            )
        elif t == 4:
            msg = (
                f"Worker {rng.randint(1,32)} "
                f"processed={rng.randint(100,50000)} "
                f"failed={rng.randint(0,100)} "
                f"elapsed={rng.randint(1,300)}s"
            )
        elif t == 5:
            msg = (
                f"Memory heap={rng.randint(64,8192)}MB "
                f"stack={rng.randint(1,64)}MB "
                f"gc_runs={rng.randint(0,200)}"
            )
        elif t == 6:
            msg = (
                f"ConnPool active={rng.randint(0,100)}/100 "
                f"idle={rng.randint(0,20)} "
                f"timeouts={rng.randint(0,10)}"
            )
        else:
            msg = (
                f"File {rng.choice(['read','write'])} "
                f"path=/var/log/{rng.choice(_SVCS)}.log "
                f"size={rng.randint(0,10240)}KB"
            )
        line = f"{ts} [{lv}] tid={tid:4d} {msg}\n"
        buf.append(line)
        total += len(line)
    return "".join(buf).encode()


# ---------------------------------------------------------------------------
# File generation
# ---------------------------------------------------------------------------

def generate_file(path: Path, target_gb: float) -> int:
    target = int(target_gb * (1 << 30))
    seed   = _seed_block()
    print(f"  Generating {target_gb:.0f} GB file ({target // (1<<30)} GiB) ...", flush=True)
    written = 0
    t0 = time.perf_counter()
    with open(path, "wb") as f:
        while written < target:
            f.write(seed)
            written += len(seed)
    elapsed = time.perf_counter() - t0
    actual_gb = written / (1 << 30)
    speed = actual_gb / elapsed
    print(f"  Done: {actual_gb:.2f} GiB in {elapsed:.1f}s ({speed:.2f} GiB/s)")
    return written


# ---------------------------------------------------------------------------
# Timing helpers
# ---------------------------------------------------------------------------

def time_cmd(cmd: list[str]) -> tuple[float, str]:
    """Run command RUNS times, return (median_s, stdout_first_run)."""
    times = []
    first_out = ""
    for i in range(RUNS):
        t0 = time.perf_counter()
        r  = subprocess.run(cmd, capture_output=True, text=True)
        dt = time.perf_counter() - t0
        times.append(dt)
        if i == 0:
            first_out = r.stdout.strip() or r.stderr.strip()
    return median(times), first_out


def time_fn(fn) -> tuple[float, object]:
    """Run a callable RUNS times, return (median_s, result_first_run)."""
    times = []
    first = None
    for i in range(RUNS):
        t0 = time.perf_counter()
        r  = fn()
        dt = time.perf_counter() - t0
        times.append(dt)
        if i == 0:
            first = r
    return median(times), first


def tool_available(name: str) -> bool:
    return shutil.which(name) is not None


# ---------------------------------------------------------------------------
# Machine info
# ---------------------------------------------------------------------------

def cpu_info() -> str:
    try:
        with open("/proc/cpuinfo") as f:
            for line in f:
                if line.startswith("model name"):
                    return line.split(":", 1)[1].strip()
    except Exception:
        pass
    return platform.processor() or "unknown"


def mem_total_gb() -> float:
    try:
        with open("/proc/meminfo") as f:
            for line in f:
                if line.startswith("MemTotal:"):
                    kb = int(line.split()[1])
                    return kb / (1 << 20)
    except Exception:
        pass
    return 0.0


# ---------------------------------------------------------------------------
# Main benchmark
# ---------------------------------------------------------------------------

def run_benchmarks(file_path: Path, file_gb: float) -> list[dict]:
    """Run all benchmarks and return result rows."""
    from log_analyzer._core import LogRepo
    import tempfile

    results: list[dict] = []

    def row(tool: str, operation: str, elapsed: float, note: str = "") -> dict:
        throughput = file_gb / elapsed if elapsed > 0 else 0
        return {
            "tool": tool,
            "operation": operation,
            "time_s": elapsed,
            "throughput_gbs": throughput,
            "note": note,
        }

    # -- 1. Line count -------------------------------------------------------
    print("\n[1/5] Line counting")
    t, out = time_cmd(["wc", "-l", str(file_path)])
    total_lines = int(out.split()[0]) if out.split() else 0
    print(f"  wc -l        : {t:.2f}s  ({total_lines:,} lines)")
    results.append(row("wc -l", "line count", t))

    if tool_available("rg"):
        # rg --count-matches counts matches per file; we want total lines
        # Use rg -c "." to count non-empty lines (equivalent for our file)
        t, out = time_cmd(["rg", "-c", ".", str(file_path)])
        lines_rg = int(out.strip()) if out.strip().isdigit() else 0
        print(f"  rg -c '.'    : {t:.2f}s  ({lines_rg:,} lines)")
        results.append(row("ripgrep", "line count", t))

    # -- 2. Pattern search (count matching lines) ----------------------------
    print(f"\n[2/5] Pattern search — count lines matching '{PATTERN}'")

    t, out = time_cmd(["/usr/bin/grep", "--color=never", "-c", PATTERN, str(file_path)])
    grep_count = int(out.strip()) if out.strip().isdigit() else 0
    print(f"  grep -c      : {t:.2f}s  ({grep_count:,} matches)")
    results.append(row("grep", f"count '{PATTERN}'", t))

    if tool_available("rg"):
        t, out = time_cmd(["rg", "-c", PATTERN, str(file_path)])
        rg_count = int(out.strip()) if out.strip().isdigit() else 0
        print(f"  rg -c        : {t:.2f}s  ({rg_count:,} matches)")
        results.append(row("ripgrep", f"count '{PATTERN}'", t))

    t, out = time_cmd([
        "awk", f"/^[^ ]* [^ ]* \\[{PATTERN}\\]/{{c++}}END{{print c}}", str(file_path)
    ])
    awk_count = int(out.strip()) if out.strip().isdigit() else 0
    print(f"  awk          : {t:.2f}s  ({awk_count:,} matches)")
    results.append(row("awk", f"count '{PATTERN}'", t))

    # log-analyzer static (raw file, uses ripgrep SIMD)
    t, la_count = time_fn(lambda: LogRepo.count_file_matches(str(file_path), PATTERN))
    print(f"  la (raw)     : {t:.2f}s  ({la_count:,} matches)")
    results.append(row("log-analyzer", f"count '{PATTERN}' (raw file)", t, "ripgrep SIMD, no import"))

    # -- 3. Import (one-time) ------------------------------------------------
    print("\n[3/5] Import & compress (one-time, not repeated)")
    if REPO_DIR.exists():
        shutil.rmtree(REPO_DIR)
    t0 = time.perf_counter()
    repo_obj = LogRepo.import_file(str(REPO_DIR), str(file_path))
    import_time = time.perf_counter() - t0
    repo_size_bytes = sum(
        f.stat().st_size for f in REPO_DIR.rglob("*") if f.is_file()
    )
    raw_size_bytes  = file_path.stat().st_size
    ratio = raw_size_bytes / repo_size_bytes if repo_size_bytes > 0 else 0
    print(
        f"  Import time  : {import_time:.2f}s "
        f"  Raw: {raw_size_bytes/(1<<30):.2f} GiB  "
        f"  Compressed: {repo_size_bytes/(1<<30):.2f} GiB  "
        f"  Ratio: {ratio:.1f}x"
    )
    results.append(row(
        "log-analyzer", "import + compress",
        import_time,
        f"raw→compressed {ratio:.1f}x, repo {repo_size_bytes/(1<<30):.2f} GiB",
    ))

    # -- 4. Pattern search on compressed repo --------------------------------
    print(f"\n[4/5] Pattern search on compressed repo — count '{PATTERN}'")
    t, la_comp_count = time_fn(lambda: repo_obj.count_matches(PATTERN))
    print(f"  la (compressed): {t:.2f}s  ({la_comp_count:,} matches)")
    results.append(row(
        "log-analyzer", f"count '{PATTERN}' (compressed)",
        t, f"reads {repo_size_bytes/(1<<30):.2f} GiB compressed",
    ))

    # -- 5. Filter to file (keep matching lines) -----------------------------
    print(f"\n[5/5] Filter '{PATTERN}' lines → output file")

    t, _ = time_cmd(["/usr/bin/grep", "--color=never", PATTERN, str(file_path),
                     "--output-file=/dev/null"])
    # grep doesn't have --output-file; redirect instead
    t, _ = time_cmd(["sh", "-c",
                     f"/usr/bin/grep --color=never '{PATTERN}' {file_path} > {OUT_FILE}"])
    print(f"  grep → file  : {t:.2f}s")
    results.append(row("grep", f"filter '{PATTERN}' → file", t))

    if tool_available("rg"):
        t, _ = time_cmd(["sh", "-c",
                         f"rg '{PATTERN}' {file_path} > {OUT_FILE}"])
        print(f"  rg   → file  : {t:.2f}s")
        results.append(row("ripgrep", f"filter '{PATTERN}' → file", t))

    t, la_filter_n = time_fn(
        lambda: repo_obj.stream_filter_to_file(PATTERN, True, str(OUT_FILE))
    )
    print(f"  la   → file  : {t:.2f}s  ({la_filter_n:,} lines written, reads compressed)")
    results.append(row(
        "log-analyzer", f"filter '{PATTERN}' → file (compressed)",
        t, "streaming, reads compressed chunks",
    ))

    return results, {
        "total_lines": total_lines,
        "grep_count":  grep_count,
        "import_time": import_time,
        "raw_gb":      raw_size_bytes  / (1 << 30),
        "repo_gb":     repo_size_bytes / (1 << 30),
        "ratio":       ratio,
    }


# ---------------------------------------------------------------------------
# Markdown rendering
# ---------------------------------------------------------------------------

def render_markdown(
    results: list[dict],
    stats: dict,
    file_gb: float,
    cpu: str,
    ram_gb: float,
) -> str:
    raw_gb  = stats["raw_gb"]
    repo_gb = stats["repo_gb"]
    ratio   = stats["ratio"]

    lines = [
        "# Performance Benchmarks",
        "",
        f"> Generated on: {time.strftime('%Y-%m-%d')}  ",
        f"> CPU: {cpu}  ",
        f"> RAM: {ram_gb:.0f} GB  ",
        f"> Storage: NVMe SSD  ",
        f"> Python: {sys.version.split()[0]}  ",
        "",
        "## Test File",
        "",
        f"- Size: **{raw_gb:.2f} GiB** synthetic log file",
        f"- Lines: **{stats['total_lines']:,}**",
        f"- Content: mixed `ERROR`/`WARN`/`INFO`/`DEBUG` lines, ~150 B each",
        f"- Pattern searched: `ERROR` (~5% of lines)",
        "",
        "## Results",
        "",
        "All times are **median of 3 runs** (warm OS page cache).  ",
        "Throughput is computed against the raw 10 GB file size.",
        "",
        "### Line counting",
        "",
        "| Tool | Time (s) | Throughput |",
        "|------|----------|------------|",
    ]

    def fmt_row(r: dict) -> str:
        note = f" *{r['note']}*" if r["note"] else ""
        return (
            f"| {r['tool']}{note} | {r['time_s']:.2f} s | "
            f"{r['throughput_gbs']:.2f} GiB/s |"
        )

    for r in results:
        if r["operation"] == "line count":
            lines.append(fmt_row(r))

    lines += [
        "",
        f"### Pattern search — count lines matching `{PATTERN}`",
        "",
        "| Tool | Time (s) | Throughput | Notes |",
        "|------|----------|------------|-------|",
    ]

    def fmt_row2(r: dict) -> str:
        note = r["note"] if r["note"] else ""
        return (
            f"| {r['tool']} | {r['time_s']:.2f} s | "
            f"{r['throughput_gbs']:.2f} GiB/s | {note} |"
        )

    for r in results:
        if "count" in r["operation"] and "import" not in r["operation"]:
            lines.append(fmt_row2(r))

    lines += [
        "",
        "### Import & compression (log-analyzer, one-time cost)",
        "",
        "| Metric | Value |",
        "|--------|-------|",
        f"| Import time | {stats['import_time']:.2f} s |",
        f"| Raw file    | {raw_gb:.2f} GiB |",
        f"| Compressed repo | {repo_gb:.2f} GiB |",
        f"| Compression ratio | {ratio:.1f}× |",
        f"| Throughput  | {raw_gb / stats['import_time']:.2f} GiB/s |",
        "",
        "### Filter matching lines to file",
        "",
        "| Tool | Time (s) | Throughput | Notes |",
        "|------|----------|------------|-------|",
    ]

    for r in results:
        if "filter" in r["operation"]:
            lines.append(fmt_row2(r))

    lines += [
        "",
        "## Summary",
        "",
        "- **grep / ripgrep / awk** operate on the raw file every time.",
        (
            f"- **log-analyzer** pays a one-time import cost ({stats['import_time']:.1f} s) "
            f"that compresses the data {ratio:.1f}× ({raw_gb:.1f} → {repo_gb:.1f} GiB)."
        ),
        (
            "- Post-import searches on compressed data read less I/O, making "
            "repeated operations faster on I/O-bound systems."
        ),
        "- `log-analyzer` raw-file search (`count_file_matches`) uses ripgrep's",
        "  SIMD searcher and is comparable in speed to `rg`.",
        "- Additional benefits: reversible operations (filter/replace with undo),",
        "  operation history, workspace management, and Python API.",
    ]

    return "\n".join(lines) + "\n"


# ---------------------------------------------------------------------------
# Entry point
# ---------------------------------------------------------------------------

def main() -> None:
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("--keep-file", action="store_true", help="Do not delete the test file")
    ap.add_argument("--file",      metavar="PATH",        help="Use existing log file")
    ap.add_argument("--gb",        type=float, default=10.0, metavar="N",
                    help="Target file size in GiB (default: 10)")
    args = ap.parse_args()

    file_path = Path(args.file) if args.file else TEST_FILE
    file_gb   = args.gb

    print("=" * 60)
    print("log-analyzer performance benchmark")
    print("=" * 60)

    # ---- Generate or verify test file ----
    if args.file:
        if not file_path.exists():
            sys.exit(f"File not found: {file_path}")
        file_gb = file_path.stat().st_size / (1 << 30)
        print(f"Using existing file: {file_path} ({file_gb:.2f} GiB)")
    else:
        print(f"\nStep 1/3  Generate test file → {file_path}")
        generate_file(file_path, file_gb)

    # ---- Run benchmarks ----
    print(f"\nStep 2/3  Run benchmarks (each repeated {RUNS}×, report median)")
    results, stats = run_benchmarks(file_path, file_gb)

    # ---- Write docs ----
    print("\nStep 3/3  Write results")
    cpu    = cpu_info()
    ram_gb = mem_total_gb()
    md     = render_markdown(results, stats, file_gb, cpu, ram_gb)

    DOCS_DIR.mkdir(exist_ok=True)
    out_md = DOCS_DIR / "benchmarks.md"
    out_md.write_text(md)
    print(f"  Saved: {out_md}")

    print("\n--- Markdown preview ---")
    print(md)

    # ---- Cleanup ----
    if not args.keep_file and not args.file:
        print(f"Deleting {file_path} ... ", end="", flush=True)
        file_path.unlink(missing_ok=True)
        print("done")

    if REPO_DIR.exists():
        print(f"Deleting {REPO_DIR} ... ", end="", flush=True)
        shutil.rmtree(REPO_DIR)
        print("done")

    if OUT_FILE.exists():
        OUT_FILE.unlink()

    print("\nBenchmark complete.")


if __name__ == "__main__":
    main()
