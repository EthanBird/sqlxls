# sqlxls

用 **SQL** 查询、清理、关联 Excel / CSV / JSON / HTTP / 目录 / 剪贴板，再导出 xlsx、csv、json。

底层是内存 SQLite。推荐写法：先 `LOAD` 成表，再写标准 `SELECT`。

- **使用手册**（CLI、选项、速查）：[`docs/USAGE.md`](docs/USAGE.md)
- **分场景教程**（Excel、GBK、分页、DATE 区间…）：[`docs/tutorials/`](docs/tutorials/README.md)
- 语法规范：[`docs/SYNTAX.md`](docs/SYNTAX.md) · 动态源：[`docs/DYNAMIC.md`](docs/DYNAMIC.md) · 架构：[`docs/ARCHITECTURE.md`](docs/ARCHITECTURE.md)

## 安装

从 [GitHub Releases](https://github.com/EthanBird/sqlxls/releases) 下载对应平台文件，放到 `PATH`：

| 平台 | 文件 |
|------|------|
| Linux x86_64 | `sqlxls-x86_64-unknown-linux-gnu` |
| macOS Apple Silicon | `sqlxls-aarch64-apple-darwin` |
| macOS Intel | `sqlxls-x86_64-apple-darwin` |
| Windows x86_64 | `sqlxls-x86_64-pc-windows-msvc.exe` |

或 `cargo build --release`（Rust 1.88+），产物在 `target/release/sqlxls`。

## 30 秒

```sql
-- analysis.sql
LOAD users  FROM 'users.xlsx' WITH (format='excel', sheet='Sheet1');
LOAD orders FROM 'orders.csv';

SELECT u.name, SUM(o.amount) AS total
FROM users u
JOIN orders o ON u.id = o.user_id
GROUP BY u.name
ORDER BY total DESC;
```

```bash
sqlxls analysis.sql --strict -o result.xlsx
```

相似源不要复制 SQL：

```sql
LOAD orders FROM '${base}/${region}/orders' WITH (format='json', json_path='data')
FOR region IN ('east', 'west');

LOAD sales FROM EACH GLOB './sales_*.csv';

LOAD daily FROM '${base}/day/${d}' WITH (format='json')
FOR d IN DATE '2024-01-01'..'2024-01-31';
```

```bash
sqlxls report.sql --strict --set base=https://api.example.com --set region=east
```

GBK 文本：`encoding='gbk'`。远程 Excel 按文件魔数识别，不依赖 `.xlsx` 后缀。

## 命令行

```text
sqlxls script.sql [--strict] [--set k=v] [-o out.xlsx] [--explain]
sqlxls "LOAD t FROM 'a.csv'; SELECT * FROM t LIMIT 8"
sqlxls -f "read('data.csv')"
```

`-o` 扩展名：`xlsx` / `csv` / `json` / `ndjson`。只有最后一条查询会输出。

完整选项与 format 表见 [使用手册](docs/USAGE.md)。

## Agent 技能包

[`skills/sqlxls-data`](skills/sqlxls-data) 给 Cursor / Claude / Codex 用。本仓库已通过 `.agents/skills/sqlxls-data` 链接。说明见 [`skills/README.md`](skills/README.md)。

## 现在做不到

就地改 xlsx、直连 Postgres/MySQL/S3/Parquet、HTTP cursor 分页、catalog / REPL。内存引擎，适合笔记本可交互规模。

## 路线图

- [x] `LOAD` + 命名选项 + `--strict`
- [x] SET / FOR / EACH / 分页 / glob / `sheet='*'`
- [x] `DATE` 区间、`parse_date` / `from_unix`、GBK、远程 Excel 魔数
- [x] GitHub Releases 多平台二进制
- [ ] `sqlxls.toml` catalog、cursor 分页、DuckDB、`read_sql`
