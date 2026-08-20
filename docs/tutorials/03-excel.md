# 03 · Excel（xlsx / xls / xlsm）

## 读一个 sheet

```sql
LOAD users FROM 'users.xlsx' WITH (
  format='excel',
  sheet='Sheet1'
);

SELECT * FROM users LIMIT 8;
```

不写 `sheet` 时用工作簿里的**第一个**表。

`skip=n`：表头之前要丢掉的行数（标题、说明、合并单元格留下的空行）：

```sql
LOAD t FROM 'report.xlsx' WITH (
  format='excel',
  sheet='汇总',
  skip=2
);
```

`str=true`：全部当文本，避免工号 `001` 变成整数 `1`：

```sql
LOAD t FROM 'ids.xlsx' WITH (format='excel', str=true);
```

## 日期

单元格里的 Excel 日期导入后是 **ISO 文本**（`2024-01-15` 或带时分秒），不是序列号。

```sql
SELECT *
FROM users
WHERE "入职日期" >= '2024-01-01';

SELECT strftime('%Y-%m', "入职日期") AS ym, COUNT(*) AS n
FROM users
GROUP BY ym;
```

若某列其实还是数字序列（别人导出的 CSV 里常见），用查询函数：

```sql
SELECT excel_serial(serial) AS d FROM t;
```

## 一个工作簿里很多同构 sheet

不要每个 sheet 一个 `LOAD`。结构相同就：

```sql
LOAD book FROM 'workbook.xlsx' WITH (
  format='excel',
  sheet='*'          -- 或 sheet='all'
);

SELECT _sheet, COUNT(*) AS n
FROM book
GROUP BY _sheet;
```

会多一列 `_sheet`。只要部分表：先 `sheet='*'` 再 `WHERE _sheet IN ('A','B')`，或只 LOAD 需要的名字。

结构不同的 sheet 不要 `*`，分开 `LOAD` 再 JOIN。

## 远程下载的 Excel

HTTP 拿到的可能是：

- `Content-Type: application/vnd.openxmlformats-officedocument.spreadsheetml.sheet`
- 或 `application/octet-stream`，URL 甚至没有 `.xlsx`

sqlxls 看**文件头魔数**（xlsx 是 ZIP 的 `PK`，xls 是 OLE），不依赖扩展名：

```sql
LOAD t FROM 'https://example.com/export' WITH (format='excel', sheet='Sheet1');
```

不写 `format='excel'` 时，只要魔数对得上也会当工作簿。HTML 登录页/错误页会报错，不会当 Excel。

Excel 是二进制，**不要**写 `encoding='gbk'`。编码选项只给 CSV/JSON/文本。

## 和 CSV 一起 JOIN

```sql
LOAD users  FROM 'users.xlsx' WITH (format='excel', sheet='Sheet1');
LOAD dept   FROM 'dept.csv';

SELECT u.name, d.department
FROM users u
LEFT JOIN dept d ON TRIM(CAST(u.id AS TEXT)) = TRIM(CAST(d.user_id AS TEXT));
```

键类型不一致时先 `TRIM` + `CAST` 成文本再比。

## 不要做的事

- 就地改公式、合并单元格、写回原 xlsx（工具只**另存** `-o`）
- 把说明性的前几行当数据：用 `skip`
- 用位置参数 `read_excel('a.xlsx', 'S', 2, 'str')` 当新脚本（`--strict` 会拒绝）

## 下一步

[CSV 与 GBK 编码](04-csv-encoding.md) · [多文件 / 多源](07-dynamic-sources.md)
