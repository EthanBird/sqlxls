# 08 · 日期：拉源用 DATE 区间，列转换用函数

两件不同的事：

1. **Source**：按日历去拉 31 个 URL / 12 个月文件 → `FOR d IN DATE '…'..'…'`  
2. **Query**：列里的字符串、时间戳、Excel 序列 → `parse_date` / `from_unix` / `excel_serial`

不要用手写 31 个 `LOAD`，也不要在 SQL 里发明循环。

## 拉源：必须有 DATE

字符串范围不加 `DATE` 会报错（避免和普通字符串搞混）：

```sql
-- 对：按日
LOAD orders FROM '${base}/orders?dt=${d}' WITH (format='json', json_path='data')
FOR d IN DATE '2024-01-01'..'2024-01-31';

SELECT _d, COUNT(*) FROM orders GROUP BY _d;

-- 对：按月文件 2024-01.csv … 2024-12.csv
LOAD sales FROM './${ym}.csv'
FOR ym IN DATE '2024-01'..'2024-12';

-- 对：紧凑数字，输出保持 YYYYMMDD
LOAD t FROM '${base}/${d}' WITH (format='json')
FOR d IN DATE 20240101..20240131;
```

```sql
-- 错：语义不明
FOR d IN '2024-01-01'..'2024-01-31'
```

两端可以来自绑定：

```sql
SET start = '2024-01-01';
SET end   = '2024-03-31';

LOAD t FROM 'https://api.example.com/day/${d}' WITH (format='json')
FOR d IN DATE '${start}'..'${end}' STEP 7;
```

```bash
sqlxls job.sql --strict --set start=2024-01-01 --set end=2024-01-31
```

### 步长

| 写法 | 含义 |
|------|------|
| （省略） | 日区间每天；月区间每月 |
| `STEP 7` | 日区间每 7 天；月区间每 7 个月 |
| `STEP MONTH` 或 `STEP 1 MONTH` | 按月。从 1 月 31 日起会钳到 2 月最后一天，3 月仍回到 31 日 |
| `STEP 2 MONTH` | 每两月 |

```sql
LOAD t FROM '${base}/${d}' WITH (format='json')
FOR d IN DATE '2024-01-01'..'2024-12-31' STEP MONTH;
```

也可用 `DATE '2024/01/01'..'2024/01/31'`（输出保持斜杠）。区间大约最多 4000 个值。

`1..12` **没有** `DATE` 时仍是整数，不会当成月份。

## 列转换

Excel 单元格日期导入后已是 ISO 文本，直接比即可。

杂乱文本：

```sql
SELECT
  parse_date(col) AS d,                 -- 2024-01-15、20240115
  parse_date(col, 'dmy') AS d_eu,       -- 15/01/2024、15-01-2024
  parse_date(col, 'mdy') AS d_us,       -- 01/15/2024
  parse_date(col, 'ymd') AS d_iso,
  parse_date(col, '%Y年%m月%d日') AS d_cn,
  parse_datetime(ts) AS dt,
  from_unix(epoch) AS dt_unix,          -- 秒；绝对值 ≥ 1e12 当毫秒
  to_unix(ts) AS epoch,
  excel_serial(n) AS d_xl
FROM t
WHERE parse_date(col, 'dmy') >= '2024-01-01';
```

失败返回 `NULL`，整句 SELECT 不会中断。

`01/02/2024` 日月都 ≤12：**必须**写 `'dmy'` 或 `'mdy'`，自动模式给 NULL，以免静默写错。

SQLite 自带仍可用：`date(x)`、`strftime('%Y-%m', x)`、`unixepoch(x)`。

## 组合：按日拉接口 + 再按月汇总

```sql
SET base = 'https://api.example.com';

LOAD orders FROM '${base}/orders?dt=${d}' WITH (
  format='json',
  json_path='data'
)
FOR d IN DATE '2024-01-01'..'2024-01-31';

SELECT strftime('%Y-%m', _d) AS ym, SUM(amount) AS total
FROM orders
GROUP BY ym;
```

`_d` 来自 `FOR d`，已经是 ISO 日期字符串。

## 下一步

[清洗与 JOIN](09-sql-cleaning.md)
