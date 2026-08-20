# 10 · 脚本、参数与导出

## 多语句 Session

一个 `.sql` 文件里可以多句，分号分隔。中间的 `LOAD` / `CREATE TABLE AS` 留在**同一次进程**的内存库里：

```sql
LOAD raw FROM 'dirty.csv' WITH (format='csv', encoding='gbk');

CREATE TABLE clean AS
SELECT
  CAST(TRIM(id) AS INTEGER) AS id,
  TRIM(name) AS name,
  CAST(REPLACE(amount, ',', '') AS REAL) AS amount
FROM raw
WHERE NULLIF(TRIM(name), '') IS NOT NULL;

SELECT name, SUM(amount) AS total
FROM clean
GROUP BY name
ORDER BY total DESC;
```

只有最后的 `SELECT` 会显示或写入 `-o`。

也可以：

```sql
CREATE TABLE users AS SELECT * FROM read('users.xlsx', format='excel', sheet='Sheet1');
```

交付请优先 `LOAD`，不要依赖查询里的 `read_*`。

## 导出

```bash
sqlxls job.sql --strict                 # 终端表
sqlxls job.sql --strict -o out.xlsx
sqlxls job.sql --strict -o out.csv
sqlxls job.sql --strict -o out.json     # JSON 数组
sqlxls job.sql --strict -o out.ndjson
```

默认**另存**，不会覆盖你的原始工作簿（除非 `-o` 故意写成同一个路径）。

## --set 与环境

脚本：

```sql
SET base = 'https://api.example.com';
LOAD t FROM '${base}/${region}/orders' WITH (format='json', json_path='data');
SELECT * FROM t LIMIT 20;
```

```bash
sqlxls job.sql --strict --set region=east
sqlxls job.sql --strict --set region=west --set base=https://other.example.com
```

`--set` 与文件里的 `SET` 一样进入绑定表。命令行适合 CI / 换环境；`SET` 适合脚本内默认值。

整数、true/false/null 会被解析成对应类型；否则当字符串。

## --explain

```bash
sqlxls job.sql --strict --explain
```

stderr 会打印 `LOAD` 展开成几个源、改写后的 SQL。展开过多或 URL 拼错时先看这里。

## --strict 与 --syntax

| | 建议 |
|--|------|
| 新脚本 | `--strict` |
| 一行在终端里试 `read_csv('a.csv')` | 可以暂时不加 |
| `--syntax=2` | 查询 `FROM` 只允许 `read()` / 已 LOAD 的表 / `mock_data` |

位置参数 `read_excel('a.xlsx', 'Sheet1', 2)` 在 `--strict` 下是错误。一律写成 `sheet='Sheet1', skip=2`。

## 探测大文件

不要第一次就把全表打到对话或终端：

```sql
LOAD t FROM 'huge.xlsx' WITH (format='excel', sheet='Sheet1');
SELECT * FROM t LIMIT 8;
```

确认列名后再在**同一脚本**里写完整聚合（同一次 `LOAD`）。两次 `sqlxls` 调用会导入两次。

## 下一步

[剪贴板、glob、假数据](11-clipboard-glob-mock.md) · [FAQ](12-faq.md)
