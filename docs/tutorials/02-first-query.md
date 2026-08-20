# 02 · 第一份脚本

sqlxls 不是「在 SQL 里发明新方言」，而是两步：

1. **Source**：`LOAD` 把文件/URL 变成表  
2. **Query**：对这张表写普通 SQL

## 黄金路径

把下面存成 `hello.sql`（把路径换成你的文件）：

```sql
LOAD t FROM 'data.csv';

SELECT *
FROM t
LIMIT 8;
```

```bash
sqlxls hello.sql --strict
```

CSV / xlsx / json 会按扩展名推断 format。推断不出时写明：

```sql
LOAD t FROM 'data.csv' WITH (format='csv');
LOAD t FROM 'book.xlsx' WITH (format='excel', sheet='Sheet1');
```

## 先看有哪些列

最后一条可以是 `PRAGMA`：

```sql
LOAD t FROM 'data.csv';
PRAGMA table_info(t);
```

或看样本与行数（各跑一次，或接受只输出最后一条）：

```sql
LOAD t FROM 'data.csv';
SELECT * FROM t LIMIT 8;
```

```sql
LOAD t FROM 'data.csv';
SELECT COUNT(*) AS n FROM t;
```

列名以探测结果为准。中文、空格、奇怪字符一律：

```sql
SELECT "销售额", "order id" FROM t;
```

`LOAD` 的**表名**必须是 ASCII：`sales` 可以，`销售` 不行。

## 一行探索（不要当交付脚本）

```bash
sqlxls "SELECT * FROM read('data.csv') LIMIT 8"
```

这是 `LOAD` 的匿名形式，会物化成临时表再查。新脚本请仍写成 `LOAD` + `--strict`。

## 类型

导入时抽样大约前 200 行，列会变成 `INTEGER` / `REAL` / `TEXT`。混型列变 TEXT。需要数字时：

```sql
SELECT CAST(amount AS REAL) AS amount FROM t;
```

空单元格是 `NULL`。空白字符串 `''` 不是 NULL，要用 `NULLIF(TRIM(x), '')`。

## 常见第一错

| 现象 | 原因 |
|------|------|
| `no such column: 销售额` | 中文列没加双引号 |
| `LOAD` 失败「表名」 | 用了中文表名 |
| 只看到最后一段结果 | 设计如此：只有最后一条查询输出 |
| `未知选项 foo` | `WITH` 里写了该 format 不认识的名字 |

## 下一步

数据是 Excel → [03](03-excel.md)；CSV/GBK → [04](04-csv-encoding.md)；JSON → [05](05-json.md)。
