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

独立轴（所有组合）：

```sql
LOAD orders FROM '${base}/${region}/${year}/orders' WITH (format='json')
FOR region IN ('east', 'west')
FOR year IN (2024, 2025);
SELECT _region, _year, COUNT(*) FROM orders GROUP BY _region, _year;
```

成对绑定（zip，不是笛卡尔）：

```sql
LOAD orders FROM '${base}/${region}/${env}/orders' WITH (format='json')
FOR (region, env) IN (
  ('east', 'prod'),
  ('west', 'staging')
);
SELECT _region, _env, COUNT(*) FROM orders GROUP BY _region, _env;
```

```sql
LOAD sales FROM EACH ('jan.csv', 'feb.csv');
LOAD sales FROM EACH GLOB './sales_*.xlsx' WITH (format='excel', sheet='Sheet1');
SELECT _source, SUM(amount) FROM sales GROUP BY _source;
```

`FOR x IN 1..12`、`IN 1..10 STEP 2`、`IN GLOB 'pat'`。

日期不要手写 31 天：

```sql
LOAD orders FROM '${base}/orders?dt=${d}' WITH (format='json')
FOR d IN DATE '2024-01-01'..'2024-01-31';          -- 日
LOAD sales FROM './${ym}.csv' FOR ym IN DATE '2024-01'..'2024-12';  -- 月
LOAD t FROM '${base}/${d}' WITH (format='json')
FOR d IN DATE '2024-01-01'..'2024-12-31' STEP MONTH;
LOAD t FROM '${base}/${d}' WITH (format='json')
FOR d IN DATE '${start}'..'${end}' STEP 7;
```

查询层转换：`parse_date(x)` / `parse_date(x,'dmy'|'mdy'|'ymd')` / `from_unix` / `to_unix` / `excel_serial`；失败为 NULL。`01/02/2024` 必须写格式。

后面的 FOR 可以用前面的变量。来源列：`FOR region` → `_region`；定位符 → `_source`。

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

`http(s)` URL 始终走 HTTP 传输，即使写了 `format='json'`。远程 CSV/JSON 默认 UTF-8；GBK 用 `encoding='gbk'` 或响应 `charset=gbk`。远程 Excel 看文件头魔数，octet-stream 也能认。

`include_source=false` 可关掉来源列。

## 禁止

- 每个文件一个表名再写 N 条相似 SELECT
- `FOR page IN 1..100` 代替 `page_param`（除非页数固定且无空页）
- 查询 SQL 里做 `${}` 替换
