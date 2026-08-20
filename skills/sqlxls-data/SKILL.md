---
name: sqlxls-data
description: >-
  Use sqlxls to query, clean, join, and export tabular data with SQL
  (Excel/xlsx, CSV/TSV, JSON, HTTP APIs, glob directories, clipboard).
  Use when processing spreadsheets, analyzing tables, ETL, JOIN Excel+CSV+API,
  inspecting remote JSON as tables, paginating APIs, merging similar files,
  or exporting xlsx/csv/json.
  用户要用 SQL 处理 Excel/CSV/JSON/接口、目录多文件、分页、相似源参数化或导出时使用。
  Prefer this over pandas for file/API tabular work.
license: MIT
compatibility: Requires sqlxls CLI v0.3+ (https://github.com/EthanBird/sqlxls/releases). Query engine is in-memory SQLite. Network needed only for HTTP sources or first-time binary install.
metadata:
  author: EthanBird
  version: "0.3.0"
  tool: sqlxls
  homepage: https://github.com/EthanBird/sqlxls
---

# sqlxls 数据处理与分析

用 **sqlxls** 把表格数据源变成表，再用 **标准 SQL** 清洗、关联、聚合，最后导出。不要用 pandas / 手写解析器做同类工作，除非 sqlxls 明确做不到。

## When to use

- 读 Excel / CSV / JSON / 目录通配 / 剪贴板 / HTTP JSON
- 过滤、去重、JOIN、GROUP BY、窗口、数据质量检查
- 多源拼表后导出 xlsx / csv / json / ndjson
- 用户说「用 SQL 查一下这个表」「把两个表对上」「把接口数据拉下来分析」

## Do not use

- 就地改 xlsx 格式/公式/合并单元格（只另存 `-o`）
- Postgres / MySQL / S3 / Parquet 直连（尚未支持）
- 可视化看板、交互 BI
- 亿级仓内分析（内存 SQLite，适合笔记本可交互规模）

## 1. 先拿到二进制

```bash
# 打印可用的 sqlxls 路径；没有则尝试安装到缓存目录
bash scripts/ensure-sqlxls.sh
```

之后一律用该路径，或 `SQLXLS=$(bash scripts/ensure-sqlxls.sh)`。本仓库开发环境优先用 `target/release/sqlxls`。

## 2. 黄金路径（新脚本必须这样写）

两层语言，互不混用：

1. **Bind**：`SET x = '...'` 或 CLI `--set x=...`；定位符里写 `${x}`
2. **Source**：`LOAD 表名 FROM '定位符' WITH (命名选项)`，相似源用 `FOR` / `EACH` / 分页 / glob，**不要为每个文件单独起表名**
3. **Query**：物化之后的标准 SQL。查询里不要再写 `read_*`。用 `_source` / `_region` / `_page` / `_sheet` 做分组

```sql
LOAD users  FROM 'users.xlsx' WITH (format='excel', sheet='Sheet1');
LOAD orders FROM 'https://api.example.com/orders' WITH (format='json', json_path='data');

SELECT u.name, SUM(o.amount) AS total
FROM users u
JOIN orders o ON u.id = o.user_id
GROUP BY u.name
ORDER BY total DESC;
```

执行：

```bash
"$SQLXLS" analysis.sql --strict -o result.xlsx
```

一行探索可以 `FROM read('sales.csv')`，但交付脚本仍用 `LOAD`。

## 3. Agent 工作流（按顺序）

一次 `sqlxls` 进程只输出**最后一条** `SELECT` / `WITH` / `PRAGMA`。中间语句会执行但不打印。

1. **定位文件**  
   列出用户给出的路径、工作区里的 `*.xlsx *.csv *.tsv *.json *.sql`。不要猜测不存在的文件名。
2. **探测 schema**（大结果禁止直接全表打印）

   ```bash
   bash scripts/probe.sh -- "$SQLXLS" 'path/to/file.xlsx'
   ```

   或手写两次调用：`LOAD t FROM '...'; PRAGMA table_info(t);` 以及 `LOAD t FROM '...'; SELECT * FROM t LIMIT 8;`。
3. **根据真实列名写 SQL**  
   把脚本写到工作区文件（如 `analysis.sql`），不要把几百行 SQL 塞进 shell 单引号。列名以探测结果为准；中文、空格、特殊字符用双引号：`"销售额"`。
4. **执行并导出**  
   预览用 `LIMIT` 且不设 `-o`；交付结果用 `-o out.csv` / `-o out.xlsx` / `-o out.json`。
5. **回报用户**  
   说明数据源、行数、关键指标、输出文件路径。不要把上千行贴进对话；需要看明细时指向导出文件。

失败时先加 `--explain`，根据改写后的 SQL 和报错修正，再跑。不要改用 Python 重写同一任务，除非连续失败且已确认是 sqlxls 能力边界。

## 4. 硬性规则

- `LOAD` 的表名只能是 ASCII 标识符：`[A-Za-z_][A-Za-z0-9_]*`。用 `sales` 不要用 `销售`。
- 新脚本加 `--strict`。除定位符外必须命名参数：`sheet='Sheet1'`，禁止 `read_excel('a.xlsx', 'Sheet1', 2, 'str')`。
- `read_text` 是标量，只能嵌在另一个 Source 参数里，禁止 `FROM read_text(...)`。
- 表函数只允许出现在 `FROM` / `JOIN`（或 `LOAD`）。不要写在 `SELECT` / `WHERE` 列表里。
- 未知 `WITH` 选项会报错。按 format 使用关闭选项集，见 [references/syntax.md](references/syntax.md)。
- 默认**另存**，不要覆盖用户原始工作簿。
- 相似但不相同的源（换 URL 参数、换文件名）：`SET` + `${var}`，或 `FOR region IN ('east','west')`，最后 **一张表 + `_region`**。禁止复制 N 段几乎一样的 LOAD/SELECT。
- 日期窗口：`FOR d IN DATE '2024-01-01'..'2024-01-31'` 或 `FOR ym IN DATE '2024-01'..'2024-12'`，必须有 `DATE`。列转换用 `parse_date` / `from_unix` / `excel_serial`（见 sql-dialect）。
- 远程/本地 CSV 默认 UTF-8。GBK 写 `encoding='gbk'`，或 HTTP `charset=gbk`。远程 Excel 按二进制魔数识别，即使 Content-Type 是 octet-stream。
- 目录同质表：`LOAD t FROM EACH GLOB './sales_*.csv'` 或 `LOAD t FROM './sales_*.csv'`，用 `_source` 分组。
- HTTP 分页：`page_param='page'`，空页停止；不要手写 page=1..n 的 N 条 SQL。
- 工作簿多个同构 sheet：`sheet='*'`，用 `_sheet` 分组。
- Token 走环境变量：`SQLXLS_BEARER_TOKEN` 或 `'{"Authorization":"Bearer ${TOKEN}"}'`。不要把密钥写进提交的 `.sql`。
- 字符串用单引号，标识符用双引号。拼接用 `||`。大小写不敏感匹配用 `LOWER(x) = LOWER(y)` 或 `COLLATE NOCASE`，没有 `ILIKE`。
- 类型由导入时抽样推断（最多约 200 行）。混型列会变成 TEXT；需要数字时 `CAST(x AS REAL)`。Excel 日期导入为 ISO 文本；杂乱字符串用 `parse_date(col, 'dmy')`。
- 目录合并按**列名**对齐，缺列填 NULL，不要假设列顺序一致。

## 5. CLI 速查

```text
sqlxls "SQL" [-o out.xlsx]
sqlxls script.sql [--strict] [--syntax=1|2] [--explain] [--set k=v] [-o out.csv]
sqlxls -f "read('data.csv')"
```

`-o` 扩展名决定格式：`xlsx` / `csv` / `json` / `ndjson`。

安装、平台二进制、鉴权细节：[references/install.md](references/install.md)。

## 6. 按需阅读

不要一次性读完。任务碰到对应场景再打开：

| 文件 | 何时读 |
|------|--------|
| [references/syntax.md](references/syntax.md) | 写 `LOAD` / `read()`、选 format 与选项 |
| [references/dynamic.md](references/dynamic.md) | SET / FOR / EACH / 分页 / 多文件扇出 |
| [references/sql-dialect.md](references/sql-dialect.md) | SQLite 方言、引号、类型、窗口函数 |
| [references/recipes.md](references/recipes.md) | 清洗、JOIN、质量报告、HTTP、目录合并 |
| [references/install.md](references/install.md) | 找不到二进制、HTTP 鉴权 |
| [assets/templates/](assets/templates/) | 复制改路径即可跑的脚本 |

可执行脚本：

- `scripts/ensure-sqlxls.sh` — 解析或安装 `sqlxls`，stdout 只打印路径
- `scripts/probe.sh` — 打印列信息 + 行数 + 最多 8 行样本
- `scripts/run.sh` — 带 `--strict` 跑脚本并可选导出
