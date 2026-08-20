# 分析配方

下面默认 `--strict`。把路径换成探测到的真实文件。模板在 `assets/templates/`。

## 预览与行数

```sql
LOAD t FROM 'data.xlsx' WITH (format='excel', sheet='Sheet1');
SELECT * FROM t LIMIT 8;
```

```sql
LOAD t FROM 'data.xlsx' WITH (format='excel');
SELECT COUNT(*) AS n FROM t;
```

## 数据质量

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
HAVING COUNT(*) > 1
ORDER BY n DESC
LIMIT 50;
```

## 清洗后导出

```sql
LOAD raw FROM 'dirty.csv' WITH (format='csv');

SELECT
  CAST(TRIM(id) AS INTEGER) AS id,
  TRIM(name) AS name,
  CAST(REPLACE(amount, ',', '') AS REAL) AS amount,
  date(ts) AS day
FROM raw
WHERE TRIM(COALESCE(name, '')) <> ''
  AND CAST(REPLACE(amount, ',', '') AS REAL) IS NOT NULL;
```

```bash
sqlxls clean.sql --strict -o cleaned.xlsx
```

## 多源 JOIN

```sql
LOAD users  FROM 'users.xlsx' WITH (format='excel', sheet='Sheet1');
LOAD orders FROM 'orders.csv';
LOAD extra  FROM 'https://api.example.com/flags' WITH (format='json', json_path='data');

SELECT u.id, u.name, SUM(o.amount) AS total, e.flag
FROM users u
JOIN orders o ON u.id = o.user_id
LEFT JOIN extra e ON e.user_id = u.id
GROUP BY u.id, u.name, e.flag
ORDER BY total DESC;
```

键对不上时先规范化：

```sql
ON TRIM(CAST(u.id AS TEXT)) = TRIM(CAST(o.user_id AS TEXT))
```

## 目录合并（月报 / 多文件）

```sql
LOAD sales FROM EACH GLOB './sales_*.xlsx' WITH (format='excel', sheet='Sheet1');
SELECT _source, strftime('%Y-%m', "日期") AS ym, SUM("金额") AS total
FROM sales
GROUP BY _source, ym
ORDER BY ym;
```

或 `'./sales_*.xlsx'`（glob 连接器，同样带 `_source`）。后续文件多出来的列会补上，缺的列是 NULL。

## HTTP JSON / 分页

```sql
LOAD orders FROM 'https://api.example.com/orders' WITH (
  format='json',
  json_path='data',
  page_param='page',
  page_to=50
);
SELECT _page, status, COUNT(*) AS n
FROM orders
GROUP BY _page, status;
```

相似环境只改参数：

```sql
SET base = 'https://api.example.com';
LOAD orders FROM '${base}/${region}/orders' WITH (format='json', json_path='data')
FOR region IN ('east', 'west');
SELECT _region, COUNT(*) FROM orders GROUP BY _region;
```

按日/月扇出（不要手写日历）：

```sql
LOAD orders FROM '${base}/orders?dt=${d}' WITH (format='json', json_path='data')
FOR d IN DATE '2024-01-01'..'2024-01-31';
SELECT _d, COUNT(*) FROM orders GROUP BY _d;

LOAD sales FROM './${ym}.csv' FOR ym IN DATE '2024-01'..'2024-12';
```

列里的日期字符串：

```sql
SELECT parse_date("日期", 'dmy') AS d, from_unix(ts) AS dt
FROM t
WHERE parse_date("日期", 'dmy') >= '2024-01-01';
```

```sql
LOAD orders FROM 'https://api.example.com/orders' WITH (
  format='json',
  json_path='data'
);
SELECT status, COUNT(*) AS n, SUM(amount) AS total
FROM orders
GROUP BY status;
```

POST：

```sql
LOAD r FROM 'https://api.example.com/query' WITH (
  format='json',
  method='POST',
  body=read_text('payload.json'),
  headers='{"Content-Type":"application/json","Authorization":"Bearer ${TOKEN}"}',
  json_path='data.items'
);
SELECT * FROM r LIMIT 20;
```

## Top N / 窗口

```sql
LOAD t FROM 'sales.csv';

SELECT *
FROM (
  SELECT
    region,
    sku,
    SUM(amount) AS total,
    RANK() OVER (PARTITION BY region ORDER BY SUM(amount) DESC) AS rk
  FROM t
  GROUP BY region, sku
) s
WHERE rk <= 10;
```

## 两个文件对账

```sql
LOAD a FROM 'left.csv';
LOAD b FROM 'right.csv';

SELECT 'only_left' AS side, a.*
FROM a LEFT JOIN b ON a.id = b.id
WHERE b.id IS NULL
UNION ALL
SELECT 'only_right' AS side, b.*
FROM b LEFT JOIN a ON b.id = a.id
WHERE a.id IS NULL;
```

金额差：

```sql
SELECT a.id, a.amount AS a_amt, b.amount AS b_amt,
       CAST(a.amount AS REAL) - CAST(b.amount AS REAL) AS diff
FROM a JOIN b ON a.id = b.id
WHERE CAST(a.amount AS REAL) != CAST(b.amount AS REAL);
```

## 透视（CASE 模拟）

```sql
SELECT
  user_id,
  SUM(CASE WHEN status = 'paid' THEN amount ELSE 0 END) AS paid,
  SUM(CASE WHEN status = 'refund' THEN amount ELSE 0 END) AS refund
FROM t
GROUP BY user_id;
```

## 假数据试跑

没有用户文件、只需验证 SQL 时：

```sql
LOAD t FROM mock_data(50, '用户名:name', '城市:city', '邮箱:email');
SELECT "城市", COUNT(*) AS n FROM t GROUP BY "城市" ORDER BY n DESC;
```
