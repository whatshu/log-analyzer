# log-analyzer

[English](README.md)

高性能日志分析工具，**Rust** 后端 + **Python** CLI 前端。面向 **10GB+** 的文本日志文件设计，将日志存储在压缩仓库中，支持完整的操作历史和撤销。内部使用 [ripgrep](https://github.com/BurntSushi/ripgrep) 的 SIMD 加速搜索引擎。

## 特性

- **压缩存储** — 日志按块分割后使用 zstd 压缩。900 MB 文件在磁盘上通常压缩至约 100 MB。
- **可逆操作** — 所有操作（过滤、替换、删除、插入、修改）均记录逆向数据，支持无限次撤销。
- **Git 风格仓库** — 每个仓库维护操作日志。仓库可克隆，用于并行分析分支。
- **流式引擎** — 逐块过滤、替换、搜索和统计，无需将整个文件加载到内存。
- **收集器 (Collector)** — 只读终端操作（参考 Java Stream Collectors），用于聚合：计数、分组、Top-N、去重、数值统计。
- **ripgrep 加速搜索** — 模式匹配使用 [grep-searcher](https://crates.io/crates/grep-searcher)，具备 ripgrep 生态的 SIMD 字面量优化。
- **多线程** — 通过 [rayon](https://github.com/rayon-rs/rayon) 并行处理块：索引构建、过滤、搜索、收集。
- **追加** — 向现有仓库增量添加新日志数据，无需重新导入。
- **Python API** — 通过 [PyO3](https://pyo3.rs) 将完整 Rust 功能暴露给 Python，可作为库使用也可通过 CLI 调用。

## 构建

需要 **Python >= 3.10** 和 **Rust 工具链**（rustc + cargo）。

提供 `build.sh` 脚本用于所有构建任务：

```bash
./build.sh                    # 仅编译 release wheel（不安装）
./build.sh --dev              # 仅编译 debug wheel
./build.sh install            # 编译 release 并安装
./build.sh install --dev      # 可编辑开发模式安装
./build.sh uninstall          # 卸载
./build.sh test               # 安装并运行全部测试
```

或手动：

```bash
pip install -e ".[dev]"       # 可编辑安装（使用 maturin）
maturin build --release       # 构建 .whl 到 target/wheels/
```

## 快速开始

```bash
# 导入日志文件（创建 "default" 仓库）
log-analyzer import server.log

# 查看前 20 行
log-analyzer view

# 过滤保留 ERROR 行
log-analyzer filter "ERROR" --keep

# 撤销过滤
log-analyzer undo

# 克隆出独立分析分支
log-analyzer repo clone default errors
log-analyzer repo use errors
log-analyzer filter "ERROR" --keep

# 切回原始——数据不受影响
log-analyzer repo use default
log-analyzer view

# 列出所有 repo
log-analyzer repo list

# 导出
log-analyzer export filtered.log
```

## CLI 命令参考

### 日志操作

| 命令 | 说明 |
|------|------|
| `import <file>` | 导入文本文件到新仓库 |
| `append <file>` | 向现有仓库追加文本文件 |
| `info` | 显示仓库元信息和操作数 |
| `view` | 查看当前状态的日志行 |
| `search <pattern>` | 搜索匹配正则的行（只读） |
| `filter <pattern>` | 保留（`--keep`）或移除（`--remove`）匹配行 |
| `replace <pattern> <replacement>` | 正则替换（支持 `$1`、`$2` 捕获组） |
| `delete <indices...>` | 按 0 起始索引删除行 |
| `insert <after> <content...>` | 在指定位置后插入行 |
| `modify <index> <content>` | 替换单行内容 |
| `undo` | 撤销上一个操作 |
| `history` | 显示操作日志 |
| `export <file>` | 将当前状态写入文件 |

### 仓库管理

| 命令 | 说明 |
|------|------|
| `repo list` | 列出所有仓库（`*` 标记当前活跃） |
| `repo use <name>` | 切换活跃仓库 |
| `repo clone <src> <dst>` | 按名称克隆仓库 |
| `repo remove <name>` | 删除仓库 |

所有日志命令支持 `--repo <name>` 指定目标仓库（默认：活跃仓库）。
工作区目录默认为 `.logrepo/`，可通过 `--workspace <path>` 修改。

## Python API

```python
from log_analyzer import LogRepo

# 导入
repo = LogRepo.import_file("./repo", "server.log")

# 或打开已有仓库
repo = LogRepo.open("./repo")

# 读取
lines = repo.read_lines(0, 10)       # 前 10 行
line  = repo.read_line(42)           # 单行

# 操作（均可撤销）
repo.filter(r"\[ERROR\]", keep=True)
repo.replace(r"\d{4}-\d{2}-\d{2}", "DATE")
repo.delete_lines([0, 5, 10])
repo.insert_lines(0, ["# 头部"])
repo.modify_line(3, "新内容")

# 撤销
repo.undo()

# 追加新数据
repo.append_file("server_day2.log")
repo.append_text("额外的一行\n")

# 收集器（只读，不修改仓库）
repo.collect_count("ERROR")                          # -> 4821
repo.collect_group_count(r"\[(\w+)\]", 1)            # -> {"ERROR": 4821, "INFO": 30102, ...}
repo.collect_top_n(r"clientId=(\d+)", 1, 5)          # -> [("1234", 500), ...]
repo.collect_unique(r"src=(\S+)", 1)                  # -> ["10.0.0.1", "10.0.0.2"]
repo.collect_numeric_stats(r"latency=(\d+)ms", 1)    # -> {"count": ..., "min": ..., ...}
repo.collect_line_stats()                              # -> {"count": ..., "avg_len": ..., ...}

# 流式处理（大文件内存友好）
repo.stream_search("ERROR", max_results=100)
repo.stream_filter_to_file("ERROR", True, "err.log")
repo.count_matches("ERROR")

# 导出与克隆
repo.export("output.log")
cloned = repo.clone_to("./repo_copy")
```

## 示例

### 分析 900 MB JSON 日志

```python
from log_analyzer import LogRepo

repo = LogRepo.import_file(".logrepo", "access.log")

# 统计错误
errors = repo.collect_count("ERROR")
total = repo.metadata().original_line_count
print(f"errors: {errors:,} / {total:,} ({errors/total*100:.1f}%)")

# 按日志级别分组
levels = repo.collect_group_count(r"\[(\w+)\]", 1)
for level, count in sorted(levels.items(), key=lambda x: -x[1]):
    print(f"  {level}: {count:,}")

# Top 10 客户端
for client_id, count in repo.collect_top_n(r"clientId=(\d+)", 1, 10):
    print(f"  clientId={client_id}: {count:,}")

# 响应时间统计
stats = repo.collect_numeric_stats(r"latency=(\d+)ms", 1)
print(f"latency: min={stats['min']:.0f} max={stats['max']:.0f} avg={stats['avg']:.1f}")
```

### 拼接分段日志

```bash
log-analyzer import logs/part1.log
log-analyzer append logs/part2.log
log-analyzer append logs/part3.log
log-analyzer info   # 显示所有分段的总行数
```

### 分支分析

```bash
# 导入基础数据
log-analyzer import access.log

# 克隆出两个独立分析分支
log-analyzer repo clone default error_analysis
log-analyzer repo clone default perf_analysis

# 分析错误
log-analyzer repo use error_analysis
log-analyzer filter '" 500 ' --keep
log-analyzer export 500_errors.log

# 分析性能（通过名称指定，无需切换）
log-analyzer filter 'slow\|timeout' --keep --repo perf_analysis

# 原始数据不受影响
log-analyzer view --repo default
```

### 敏感数据脱敏

```bash
log-analyzer import access.log
log-analyzer replace '\d{1,3}\.\d{1,3}\.\d{1,3}\.\d{1,3}' 'X.X.X.X'
log-analyzer replace 'user=\w+' 'user=REDACTED'
log-analyzer export anonymized.log
```

## 性能基准

测试文件：897 MB 日志（215 万行）。一次性导入：**~550 ms**。

运行基准测试：`python3 benchmarks/bench.py [LOG_FILE]`

### 性能对比

| 任务 | grep | ripgrep | sed | awk | Python | log-analyzer |
|------|------|---------|-----|-----|--------|--------------|
| 计数匹配 | 200 ms | **116 ms** | — | 701 ms | 327 ms | 178 ms |
| 过滤写文件 | 275 ms | **181 ms** | — | 750 ms | 405 ms | 557 ms |
| 正则替换 | — | 948 ms | **640 ms** | — | 7.17 s | 967 ms |
| 分组计数 | — | 1.26 s* | — | — | 938 ms | **250 ms** |

*\* rg \| sort \| uniq -c 管道*

**要点：**

- **计数/搜索**：log-analyzer 约为 ripgrep 的 1.5 倍（内部使用 ripgrep 的 grep-searcher），快于 grep、awk、Python。
- **正则替换**：与 sed、ripgrep 同级；比 Python 快 7 倍。
- **聚合（分组、Top-N、统计）**：**log-analyzer 最快** — 比 Python 快 3.7 倍，比 rg|sort|uniq 管道快 5 倍。这是相对 Unix 工具的核心优势。

### 易用性对比

| 功能 | grep/rg/sed/awk | log-analyzer |
|------|-----------------|--------------|
| 计数匹配 | `grep -c` / `rg -c` | `collect_count(pattern)` |
| 过滤写文件 | `grep pattern > out` | `stream_filter_to_file()` |
| 正则替换 | `sed -E 's/.../.../'` | `stream_replace_to_file()` |
| 分组计数 | `rg \| sort \| uniq -c` | `collect_group_count()` |
| Top-N 频率 | `... \| sort \| head -N` | `collect_top_n()` |
| 数值统计 | awk（手写脚本） | `collect_numeric_stats()` |
| 撤销操作 | 不可能 | `undo()` |
| 操作历史 | 不可能 | `history()` |
| 压缩存储 | 无（原始文件） | zstd 分块 |
| 追加/拼接文件 | `cat >> file` | `append_file()` |
| 分支分析 | `cp -r` + 手动 | `clone_to()` |
| 随机行访问 | `sed -n 'Np'`（慢） | `read_line(N)`（有索引） |
| Python API | 仅 subprocess | 原生 import |

## 项目结构

```
log-analyzer/
├── build.sh                构建/安装/卸载脚本
├── Cargo.toml              Rust 包配置
├── pyproject.toml          Python 包配置（maturin）
│
├── src/                    Rust 核心（通过 PyO3 编译为 Python 扩展）
│   ├── lib.rs              PyO3 模块入口
│   ├── bindings.rs         Python 类/方法绑定
│   ├── error.rs            错误类型
│   ├── repo/               日志仓库
│   │   ├── mod.rs          LogRepo：创建、打开、追加、操作、撤销
│   │   ├── workspace.rs    Workspace：命名仓库管理、克隆、迁移
│   │   ├── storage.rs      ChunkStorage：zstd 压缩块 I/O
│   │   └── metadata.rs     RepoMetadata：UUID、时间戳、统计
│   ├── index/              行索引
│   │   ├── mod.rs          LineIndex：基于块的行查找
│   │   └── builder.rs      IndexBuilder：并行换行符扫描
│   ├── operator/           可逆操作符
│   │   ├── mod.rs          Operation 枚举、InverseData、分发
│   │   ├── filter.rs       正则过滤（保留/移除）
│   │   ├── replace.rs      正则替换（支持捕获组）
│   │   └── crud.rs         DeleteLines、InsertLines、ModifyLine
│   └── engine/             流式处理引擎
│       ├── mod.rs          共享块读取工具
│       ├── fast.rs         ripgrep 驱动的 SIMD 搜索（grep-searcher）
│       ├── stream.rs       LineStream：逐块迭代器
│       ├── processor.rs    ChunkedProcessor：流式过滤/替换/搜索
│       └── collector.rs    Collector：计数、分组、Top-N、去重、数值统计
│
├── python/log_analyzer/    Python 包
│   ├── __init__.py         导出 LogRepo、RepoMetadata、OperationRecord
│   └── cli.py              Click CLI
│
├── tests/                  测试套件（80 Rust + 101 Python = 181 个测试）
├── benchmarks/             性能基准测试
│   └── bench.py            与 grep、rg、sed、awk、Python 对比
└── .claude/                AI agent skills
```

## 测试

```bash
cargo test                  # Rust 测试（80 个）
pytest tests/ -v            # Python 测试（101 个）
./build.sh test             # 构建、安装并运行全部测试
```

## 许可证

MIT
