# 09 · 清洗、JOIN、窗口（查询层）

物化之后就是内存 **SQLite**。按 SQLite 写，不要按 Postgres / DuckDB 抄 `ILIKE`、`GENERATE_SERIES`。

方言速查：[sql-dialect](../../skills/sqlxls-data/references/sql-dialect.md)（与本文重复的部分以手册为准）。

## 标识符

| | 写法 |
|--|------|
| 字符串 | `'Sheet1'` |
| 表/列 | `"销售额"`、`"order id"` |
| 拼接 | `a \|\| b` |
| 注释 | `-- ...` 或 `/* ... */` |

表名（`LOAD` 的那个）不能中文。列名可以。

## 空值与空白

```sql
NULLIF(TRIM("备注"), '')
COALESCE("电话", "备用电话", '')
```

## 类型

```sql
CAST(amount AS REAL)
CAST(id AS INTEGER)
CAST(x AS TEXT)
typeof(amount)
```

千分位：`CAST(REPLACE(amount, ',', '') AS REAL)`。

## 质量报告

```sql
LOAD t FROM 'data.csv';

SELECT
  COUNT(*) AS n,
  COUNT(DISTINCT id) AS uniq_id,
  SUM(CASE WHEN id IS NULL THEN 1 ELSE 0 END) AS id_null,
  SUM(CASE WHEN TRIM(COALESCE(name, '')) = '' THEN 1 ELSE 0 END) AS name_blank,
  SUM(CASE WHEN CAST(amount AS TEXT) GLOB '*[^0-9.]*' THEN 1 ELSE 0 END) AS amount_non_numeric
FROM t;
```

重复键：

```sql
SELECT id, COUNT(*) AS n
FROM t
GROUP BY id
HAVING COUNT(*) >= 2
ORDER BY n DESC
LIMIT 50;
```

## JOIN

```sql
LOAD users  FROM 'users.xlsx' WITH (format='excel', sheet='Sheet1');
LOAD orders FROM 'orders.csv';
LOAD extra  FROM 'https://api.example.com/flags' WITH (format='json', json_path='data');

SELECT
  u.id,
  u.name,
  SUM(o.amount) AS total,
  e.flag
FROM users u
JOIN orders o ON u.id = o.user_id
LEFT JOIN extra e ON e.user_id = u.id
GROUP BY u.id, u.name, e.flag
ORDER BY total DESC;
```

键对不上：

```sql
ON TRIM(CAST(u.id AS TEXT)) = TRIM(CAST(o.user_id AS TEXT))
```

没有 `FULL OUTER JOIN`：用 `LEFT JOIN` + `UNION` 补。没有 `ILIKE`：`LOWER(x) = LOWER(y)` 或 `COLLATE NOCASE`。`LIKE` 默认对 ASCII 大小写不敏感。

## 窗口：每组 Top N

```sql
LOAD t FROM 'sales.csv';

SELECT *
FROM (
  SELECT
    region,
    sku,
    amount,
    ROW_NUMBER() OVER (PARTITION BY region ORDER BY amount DESC) AS rn
  FROM t
)
WHERE rn <= 10;
```

## CTE

```sql
WITH ok AS (
  SELECT * FROM t WHERE status = 'ok'
)
SELECT cat, COUNT(*) FROM ok GROUP BY cat;
```

## 过滤、排序、分页结果

```sql
SELECT * FROM t
WHERE id > 10
ORDER BY id DESC
LIMIT 50 OFFSET 0;
```

这是**查询结果**分页，不是 HTTP `page_param`。

## 下一步

[脚本、导出、--set](10-export-and-scripts.md)
