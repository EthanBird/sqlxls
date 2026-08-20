# 07 · 相似源：一份 SELECT，多个文件/URL

不要为 `east.csv` / `west.csv` 各写一段几乎一样的 `LOAD` + `SELECT`。默认策略：

**展开 N 个物理源 → 按列名 UNION → 一张表 + 来源列。**

设计说明：[DYNAMIC.md](../DYNAMIC.md)。

## SET 与 ${var}

```sql
SET base = 'https://api.example.com';
SET region = 'east';

LOAD orders FROM '${base}/${region}/orders' WITH (
  format='json',
  json_path='data'
);

SELECT status, COUNT(*) FROM orders GROUP BY status;
```

```bash
sqlxls report.sql --strict --set region=west --set base=https://api.example.com
```

查找顺序：**SET / FOR / `--set` → 环境变量**。定位符里缺变量会报错；`headers` 里缺则留空（方便 `${TOKEN}`）。

查询 SQL **不会**做 `${}` 替换。维度用 `_region`、`_source` 这些列。

## FOR 列表

```sql
LOAD orders FROM '${base}/${region}/orders' WITH (format='json', json_path='data')
FOR region IN ('east', 'west', 'north');

SELECT _region, SUM(amount) AS total
FROM orders
GROUP BY _region;
```

`FOR region` 自动加列 `_region`。

## 两个变量：组合还是成对？

**独立轴、要所有组合**（多个 `FOR`，嵌套）：

```sql
LOAD orders FROM '${base}/${region}/${year}/orders' WITH (format='json')
FOR region IN ('east', 'west')
FOR year  IN (2024, 2025);
-- 4 个源：east/2024、east/2025、west/2024、west/2025
-- 列 _region、_year
```

**本来就是一对一对出现**（zip，一个 `FOR` 里写元组）：

```sql
LOAD orders FROM '${base}/${region}/${env}/orders' WITH (format='json')
FOR (region, env) IN (
  ('east', 'prod'),
  ('west', 'staging')
);
-- 只有 2 个源，不会出现 east+staging
```

也可 `FOR region, env IN ((...), (...))`。范围和 GLOB 只能绑一个变量。

## EACH：一组定位符

```sql
LOAD sales FROM EACH (
  'jan.csv',
  'feb.csv',
  'https://api.example.com/mar'
) WITH (format='csv');   -- URL 仍走 HTTP
```

```sql
LOAD sales FROM EACH GLOB './sales_*.xlsx' WITH (
  format='excel',
  sheet='Sheet1'
);

SELECT _source, SUM(amount) FROM sales GROUP BY _source;
```

路径本身带 `*` 也可以：`LOAD t FROM './sales_*.csv';`（glob 连接器）。

## 整数范围

```sql
LOAD t FROM './month_${n}.csv'
FOR n IN 1..12;

LOAD t FROM './batch_${n}.csv'
FOR n IN 1..10 STEP 2;
```

月份文件若是 `2024-01.csv` 这种，用日期区间（下一篇），不要用 `1..12` 再自己补零（除非文件名就是 `1.csv`）。

## 分页、全 sheet：用连接器，不用 FOR

| 场景 | 写法 |
|------|------|
| HTTP 多页直到空 | `page_param='page'` |
| 工作簿同构多表 | `sheet='*'` |
| 目录同构多文件 | glob / `EACH GLOB` |

反模式：`FOR page IN 1..100` 在空页后还继续打。

## 关掉来源列

```sql
LOAD t FROM './sales_*.csv' WITH (include_source=false);
```

## 下一步

日期窗口必须写 `DATE`：[08-dates.md](08-dates.md)
