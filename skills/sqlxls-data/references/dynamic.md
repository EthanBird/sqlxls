# 动态源（Agent 速查）

相似源不要复制 SQL。展开发生在 Source 层，默认 UNION 成一张表。

## SET / --set

```sql
SET base = 'https://api.example.com';
LOAD t FROM '${base}/${region}/orders' WITH (format='json', json_path='data');
```

```bash
sqlxls q.sql --set region=east --strict
```

`${name}`：SET / FOR / --set → 环境变量。定位符未定义会报错。

## FOR / EACH

```sql
LOAD orders FROM '${base}/${region}/orders' WITH (format='json')
FOR region IN ('east', 'west');
SELECT _region, COUNT(*) FROM orders GROUP BY _region;
```

```sql
LOAD sales FROM EACH ('jan.csv', 'feb.csv');
LOAD sales FROM EACH GLOB './sales_*.xlsx' WITH (format='excel', sheet='Sheet1');
SELECT _source, SUM(amount) FROM sales GROUP BY _source;
```

`FOR x IN 1..12`、`IN 1..10 STEP 2`、`IN GLOB 'pat'`。多个 FOR 嵌套（后者可用前者变量）。

来源列：`FOR region` → `_region`；定位符 → `_source`。

## 连接器展开

```sql
-- 分页直到空页
LOAD t FROM 'https://api.example.com/items' WITH (
  format='json', json_path='data', page_param='page', page_from=1, page_to=50
);
-- _page

-- 全部 sheet
LOAD t FROM 'book.xlsx' WITH (format='excel', sheet='*');
-- _sheet

-- 目录
LOAD t FROM './logs_*.csv';
-- _source
```

`http(s)` URL 始终走 HTTP 传输，即使写了 `format='json'`。

`include_source=false` 可关掉来源列。

## 禁止

- 每个文件一个表名再写 N 条相似 SELECT
- `FOR page IN 1..100` 代替 `page_param`（除非页数固定且无空页）
- 查询 SQL 里做 `${}` 替换
