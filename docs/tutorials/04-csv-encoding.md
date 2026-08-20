# 04 · CSV 与编码

国内报表经常是 **GBK**，而工具默认按 **UTF-8** 读。编码不对时会直接报错并提示写 `encoding='gbk'`，不会 silently 变成乱码。

## 基本读取

```sql
LOAD sales FROM 'sales.csv';
-- 或
LOAD sales FROM 'sales.csv' WITH (format='csv');
```

TSV：

```sql
LOAD t FROM 'sales.tsv';
-- 或显式
LOAD t FROM 'sales.txt' WITH (format='csv', delim='\t');
```

其它分隔符：`delim=';'`、`delim='|'`。也可写 `sep`。

表头前有说明行：

```sql
LOAD t FROM 'sales.csv' WITH (format='csv', skip=1);
```

没有表头、第一行就是数据：

```sql
LOAD t FROM 'raw.csv' WITH (format='csv', header=false);
LOAD t FROM 'raw.csv' WITH (
  format='csv',
  header=false,
  columns='id,name,金额'
);
```

只写 `columns=` **不会**变成无表头：第一行仍会被当成表头读掉。无表头必须加 `header=false`。文件有表头只想改名：`columns='a,b'` 即可。

工号/电话不要被当成数字：

```sql
LOAD t FROM 'sales.csv' WITH (format='csv', str=true);
```

## GBK / GB18030

本地文件：

```sql
LOAD t FROM '国标报表.csv' WITH (
  format='csv',
  encoding='gbk'
);

SELECT "姓名", "金额" FROM t LIMIT 8;
```

也支持 `gb2312`、`gb18030`、`big5`、`utf-16le`、`utf-16be`。`encoding` 与 `charset` 同义。

UTF-8 带 BOM 的文件按 UTF-8 读即可，不必写 encoding。

## 远程 CSV

```sql
-- 响应头带 charset=gbk 时，可以不写 encoding
LOAD t FROM 'https://example.com/export.csv' WITH (format='csv');

-- Content-Type 是 octet-stream、实际是 GBK 文本：
LOAD t FROM 'https://example.com/export' WITH (
  format='csv',
  encoding='gbk'
);
```

`http(s)` 走 HTTP 连接器；`format='csv'` 表示按 CSV **解码**。

## 目录里一堆月报

```sql
LOAD sales FROM EACH GLOB './sales_*.csv' WITH (
  format='csv',
  encoding='gbk'
);

SELECT _source, SUM(CAST("金额" AS REAL)) AS total
FROM sales
GROUP BY _source;
```

缺的列填 NULL，按**列名**对齐，不按位置。

## 清洗后再分析

```sql
LOAD raw FROM 'dirty.csv' WITH (format='csv', encoding='gbk');

SELECT
  CAST(TRIM(id) AS INTEGER) AS id,
  TRIM(name) AS name,
  CAST(REPLACE(amount, ',', '') AS REAL) AS amount
FROM raw
WHERE NULLIF(TRIM(name), '') IS NOT NULL;
```

导出仍然是 UTF-8（xlsx/csv/json 输出侧）。用 Excel 打开导出的 csv 若中文不对，用 xlsx 或在 Excel 里选 UTF-8。

## 下一步

[JSON](05-json.md) · [HTTP](06-http-api.md)
