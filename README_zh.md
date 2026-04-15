# log-analyzer

[English](README.md)

高性能日志分析工具，**Rust** 后端 + **Python** CLI 前端。面向 **10GB+** 的文本日志文件设计，将日志存储在压缩仓库中，支持完整的操作历史和撤销。

## 特性

- **压缩存储** — 日志按块分割后使用 zstd 压缩。900 MB 文件在磁盘上通常压缩至约 100 MB。
- **可逆操作** — 所有操作（过滤、替换、删除、插入、修改）均记录逆向数据，支持无限次撤销。
- **Git 风格仓库** — 每个仓库维护操作日志。仓库可克隆，用于并行分析分支。
- **流式引擎** — 逐块过滤、替换、搜索和统计，无需将整个文件加载到内存。
- **收集器 (Collector)** — 只读终端操作（参考 Java Stream Collectors），用于聚合：计数、分组、Top-N、去重、数值统计。
- **多线程** — 通过 [rayon](https://github.com/rayon-rs/rayon) 并行处理块：索引构建、过滤、搜索、收集。
- **追加** — 向现有仓库增量添加新日志数据，无需重新导入。
- **Python API** — 通过 [PyO3](https://pyo3.rs) 将完整 Rust 功能暴露给 Python，可作为库使用也可通过 CLI 调用。

## 安装

需要 Python >= 3.10 和 Rust 工具链。

```bash
pip install -e ".[dev]"
```

使用 [maturin](https://github.com/PyO3/maturin) 编译 Rust 核心并安装 `log-analyzer` CLI。

## 快速开始

```bash
# 导入日志文件
log-analyzer import server.log

# 查看前 20 行
log-analyzer view

# 过滤保留 ERROR 行
log-analyzer filter "ERROR" --keep

# 查看操作历史
log-analyzer history

# 撤销过滤
log-analyzer undo

# 导出当前状态
log-analyzer export filtered.log
```

## CLI 命令参考

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
| `clone <dest>` | 克隆仓库用于并行分析 |

所有命令支持 `--repo <path>`（默认：`.logrepo/`）。

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

repo = LogRepo.import_file(".logrepo", "log/log.json")

# 统计 "sending" 消息
sending = repo.collect_count("sending")
total = repo.metadata().original_line_count
print(f"sending: {sending:,} / {total:,} ({sending/total*100:.1f}%)")

# 按目标分组
targets = repo.collect_group_count(r"sending to legacy (\w+)", 1)
for target, count in sorted(targets.items(), key=lambda x: -x[1]):
    print(f"  -> {target}: {count:,}")

# Top 10 客户端
for client_id, count in repo.collect_top_n(r"clientId=(\d+)", 1, 10):
    print(f"  clientId={client_id}: {count:,}")

# payloadLen 统计
stats = repo.collect_numeric_stats(r"payloadLen=(\d+)", 1)
print(f"payloadLen: min={stats['min']:.0f} max={stats['max']:.0f} avg={stats['avg']:.1f}")
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
# 创建基础仓库
log-analyzer import access.log --repo base

# 克隆进行两个独立分析
log-analyzer clone error_analysis --repo base
log-analyzer clone perf_analysis  --repo base

# 分析错误
log-analyzer filter '" 500 ' --keep --repo error_analysis
log-analyzer export 500_errors.log  --repo error_analysis

# 分析性能
log-analyzer filter 'slow\|timeout' --keep --repo perf_analysis
```

### 敏感数据脱敏

```bash
log-analyzer import access.log
log-analyzer replace '\d{1,3}\.\d{1,3}\.\d{1,3}\.\d{1,3}' 'X.X.X.X'
log-analyzer replace 'user=\w+' 'user=REDACTED'
log-analyzer export anonymized.log
```

## 设计思路

该项目最初的设计目标：

1. **日志仓** — 保存日志原文及元信息，使用压缩存储。支持复制，记录操作用于回退，参考 git 的实现。
2. **操作符** — 应用于日志仓，所有操作符均可反向操作用于撤回。支持格式化语句和正则表达式，按行处理文本。
3. **性能** — 面向 >10GB 文本日志，使用 Rust 多线程设计。参考 klogg 等高性能文本处理软件。
4. **AI 优化** — 提供 Claude Code skills，便于 AI agent 使用。

## 架构

```
src/                        Rust 核心（通过 PyO3 编译为 Python 扩展）
├── lib.rs                  PyO3 模块入口
├── bindings.rs             Python 类/方法绑定
├── error.rs                错误类型
├── repo/                   日志仓库
│   ├── mod.rs              LogRepo：创建、打开、追加、操作、撤销
│   ├── storage.rs          ChunkStorage：zstd 压缩块 I/O
│   └── metadata.rs         RepoMetadata：UUID、时间戳、统计
├── index/                  行索引
│   ├── mod.rs              LineIndex：基于块的行查找
│   └── builder.rs          IndexBuilder：并行换行符扫描
├── operator/               可逆操作符
│   ├── mod.rs              Operation 枚举、InverseData、分发
│   ├── filter.rs           正则过滤（保留/移除）
│   ├── replace.rs          正则替换（支持捕获组）
│   └── crud.rs             DeleteLines、InsertLines、ModifyLine
└── engine/                 流式处理引擎
    ├── mod.rs              共享块读取工具
    ├── stream.rs           LineStream：逐块迭代器
    ├── processor.rs        ChunkedProcessor：流式过滤/替换/搜索
    └── collector.rs        Collector：计数、分组、Top-N、去重、数值统计

python/log_analyzer/        Python 包
├── __init__.py             导出 LogRepo、RepoMetadata、OperationRecord
└── cli.py                  Click CLI

tests/                      测试套件（61 Rust + 85 Python = 146 个测试）
```

## 测试

```bash
cargo test                  # Rust 测试
pytest tests/ -v            # Python 测试
cargo test && pytest tests/ -v  # 全部
```

## 许可证

MIT
