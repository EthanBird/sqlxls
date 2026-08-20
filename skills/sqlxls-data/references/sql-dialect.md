# 查询层：SQLite 方言

物化之后就是内存 SQLite。按 SQLite 写，不要按 DuckDB / Postgres 写。

## 标识符与字符串

| 用途 | 写法 |
|------|------|
| 字符串 | `'Sheet1'` |
| 标识符（表/列） | `"销售额"`、`"order id"` |
| 转义双引号 | `"say ""hi"""` |
| 拼接 | `a \|\| b` |
| 注释 | `-- ...` 或 `/* ... */` |

`LOAD` 表名不能用中文。列名可以，必须加双引号。

探测到的列名原样使用。空表头会变成 `col_0`；重名加 `_1` 后缀。

## 类型

导入抽样最多约 200 行，列类型为 `INTEGER` / `REAL` / `TEXT`。`str=true` 则全 TEXT。Bool 存成 0/1。

```sql
CAST(amount AS REAL)
CAST(id AS INTEGER)
CAST(x AS TEXT)
typeof(amount)          -- 单值诊断
```

Excel 日期是 **ISO 文本**（不是 Excel 序列）。用文本比较、SQLite 自带函数，或 sqlxls 注册的转换函数：

```sql
WHERE "日期" >= '2024-01-01'
SELECT date("日期") AS d, strftime('%Y-%m', "日期") AS ym

-- 杂乱字符串 / 时间戳 / 仍是序列的列
SELECT
  parse_date(col) AS d,                 -- 2024-01-15、20240115；日月都 ≤12 时不要靠自动
  parse_date(col, 'dmy') AS d_eu,       -- 15/01/2024
  parse_date(col, 'mdy') AS d_us,       -- 01/15/2024
  parse_date(col, '%Y年%m月%d日') AS d_cn,
  parse_datetime(ts) AS dt,
  from_unix(epoch) AS dt_unix,          -- 秒；|x| ≥ 1e12 当毫秒
  to_unix(ts) AS epoch,
  excel_serial(n) AS d_xl               -- 与 xlsx 导入相同
FROM t;
```

解析失败返回 NULL，不报错。未知格式名（不是 `iso`/`ymd`/`dmy`/`mdy`/`yyyymmdd` 也没有 `%`）会报错。

按日/月拉多个源不要手写列表，用 Source 层区间：`FOR d IN DATE '2024-01-01'..'2024-01-31'`，见 [dynamic.md](dynamic.md)。

空单元格是 NULL。空白字符串不是 NULL：

```sql
NULLIF(TRIM("备注"), '')
COALESCE("电话", "备用电话", '')
```

## 常用 SQL（引擎已有）

```sql
-- 过滤 / 排序 / 分页
SELECT * FROM t WHERE id > 10 ORDER BY id DESC LIMIT 50 OFFSET 0;

-- 聚合
SELECT cat, COUNT(*) AS n, SUM(amt) AS total, AVG(amt) AS avg_amt
FROM t GROUP BY cat HAVING COUNT(*) >= 3;

-- 窗口
SELECT *, ROW_NUMBER() OVER (PARTITION BY user_id ORDER BY ts DESC) AS rn
FROM t;

-- CTE
WITH x AS (SELECT * FROM t WHERE status = 'ok')
SELECT COUNT(*) FROM x;

-- 去重
SELECT DISTINCT user_id, day FROM t;
SELECT * FROM t GROUP BY id;          -- 每组任意一行，慎用
```

大小写：

```sql
WHERE LOWER(name) = LOWER('Ada')
WHERE name = 'ada' COLLATE NOCASE
WHERE name LIKE '%ada%'              -- LIKE 默认大小写不敏感（ASCII）
```

没有 `ILIKE`。没有 `FULL OUTER JOIN`（用 `LEFT JOIN` + `UNION` 补）。`GENERATE_SERIES` 没有。

字符串：`TRIM` `LENGTH` `SUBSTR` `REPLACE` `INSTR` `UPPER` `LOWER` `printf`。

条件：`CASE WHEN ... THEN ... ELSE ... END`。

## 探 schema

```sql
LOAD t FROM 'file.csv';
PRAGMA table_info(t);
```

`PRAGMA` 可作为最后一条输出。`SELECT * FROM t LIMIT 8` 看样本。`SELECT COUNT(*) AS n FROM t` 看行数。三次调用 = 三次导入；小文件无所谓，大文件先 `LIMIT` 确认列，再在同一脚本里一次 `LOAD` 做完整分析。

## 输出

只有最后一条查询会打印或写入 `-o`。前面的 `LOAD` / `CREATE TABLE` / `INSERT` 只改 Session。

```bash
sqlxls q.sql                 # 终端表
sqlxls q.sql -o out.xlsx
sqlxls q.sql -o out.csv
sqlxls q.sql -o out.json     # JSON 数组
sqlxls q.sql -o out.ndjson
sqlxls q.sql --explain       # stderr 打印改写后的 SQL
```
