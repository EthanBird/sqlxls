# 01 · 安装与命令行

## 下载二进制

打开 [GitHub Releases](https://github.com/EthanBird/sqlxls/releases)，按机器选一个文件：

| 你的系统 | 下载 |
|----------|------|
| Linux x86_64 | `sqlxls-x86_64-unknown-linux-gnu` |
| macOS M 系列 | `sqlxls-aarch64-apple-darwin` |
| macOS Intel | `sqlxls-x86_64-apple-darwin` |
| Windows x86_64 | `sqlxls-x86_64-pc-windows-msvc.exe` |

Linux / macOS：

```bash
chmod +x sqlxls-aarch64-apple-darwin
sudo mv sqlxls-aarch64-apple-darwin /usr/local/bin/sqlxls
sqlxls --version
```

Windows：把 `.exe` 放到已在 `PATH` 里的目录，或在当前目录执行 `.\sqlxls-x86_64-pc-windows-msvc.exe --help`。

## 从源码编译

需要 [Rust](https://rustup.rs/) 1.88 或更新：

```bash
git clone https://github.com/EthanBird/sqlxls.git
cd sqlxls
cargo build --release
./target/release/sqlxls --help
```

## 第一次运行

```bash
sqlxls --help
```

三种输入方式：

```bash
# 1. 文件（推荐，可多语句）
sqlxls analysis.sql --strict -o out.xlsx

# 2. 命令行里直接写 SQL
sqlxls "LOAD t FROM 'data.csv'; SELECT COUNT(*) AS n FROM t"

# 3. 只跑一个表函数（调试 API / 剪贴板）
sqlxls -f "read('data.csv')"
```

## 常用选项

| 选项 | 什么时候用 |
|------|------------|
| `--strict` | 永远建议打开。禁止 `read_excel('a.xlsx', 'Sheet1', 2)` 这种位置参数超载 |
| `-o out.xlsx` | 把**最后一条查询**写成文件。扩展名：`xlsx` / `csv` / `json` / `ndjson` |
| `--set region=east` | 给脚本里 `${region}` 赋值，可重复 |
| `--explain` | 先看展开了几个源、SQL 改写成什么样，再执行 |
| `--syntax=2` | 查询的 `FROM` 里不允许 `read_excel`，只允许 `read()` 或已 `LOAD` 的表 |

不设 `-o` 时，结果打印成终端表格。

## 一次进程只输出最后一条

```sql
LOAD t FROM 'a.csv';          -- 执行但不打印
SELECT COUNT(*) FROM t;       -- 不会出现在 -o 里
SELECT * FROM t LIMIT 8;      -- 只有这一条是输出
```

要看行数，就把 `COUNT(*)` 放在最后，或分两次调用。

## 下一步

[第一份脚本：LOAD + SELECT](02-first-query.md)
