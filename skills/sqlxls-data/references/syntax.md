# sqlxls Source 语法（v0.3 / syntax=1）

查询必须是标准 SQL。数据源只用这一套封闭语言。

## LOAD

```sql
LOAD [TABLE] ident FROM 'locator' WITH (name=value, ...);
LOAD ident FROM read('locator', format='csv', delim=',');
```

- `ident`：ASCII 表名，之后 SQL 里用这个名字。
- `locator`：文件路径、`https://` URL、含 `*`/`?` 的 glob、`clip:` / `clipboard:`。
- `WITH` **只接受命名参数**。没有「第三位到底是 skip 还是 str」。
- 动态扇出见 [dynamic.md](dynamic.md)：`SET` / `FOR` / `EACH` / 分页。

推荐：先 `LOAD` 再 `SELECT`。`FROM read(...)` 是匿名糖，desugar 成临时表。

## 定位符 → 默认 format

| locator | format |
|---------|--------|
| `*.xlsx` `*.xls` `*.xlsm` | `excel` |
| `*.csv` `*.tsv` | `csv` |
| `*.json` | `json` |
| `http://` `https://` | `json`（再按 Content-Type / 魔数纠正） |
| 含 `*` `?` 或 `glob:` 前缀 | `glob` |
| `clip:` `clipboard:` | `clipboard` |

显式 `format=` 覆盖推断。推断不出时必须写 `format`。

## 各 format 关闭选项集

未列出的名字会报错，不要指望被忽略。

| format | 选项 | 说明 |
|--------|------|------|
| `excel` | `sheet`, `skip`, `str` | 默认第一个 sheet；`skip` 为跳过的表头前行数；`str=true` 全当文本 |
| `csv` | `delim`/`sep`, `skip`, `str` | TSV 用 `delim='\t'` |
| `json` | `json_path`/`path` | 点路径，如 `data.items`；也可 `$.data.items` |
| `http` | `method`, `body`/`payload`, `headers`, `json_path` | `headers` 是 JSON 对象字符串；`${ENV}` 会展开 |
| `glob` | 与被匹配文件的 format 相同，外加按扩展名分发 | 多文件按列名 UNION |
| `clipboard` | `delim`, `str` | |
| `text` | （无） | 标量，不能当表 |

`read_text('path')` 只能作为另一个 Source 的参数（例如 POST body）。

## 规范 `read()` 与糖

```sql
-- 规范
FROM read('a.xlsx', format='excel', sheet='Sheet1', skip=1)
FROM read('https://api.example.com/orders', format='json', json_path='data')

-- 糖（syntax=1 允许；--strict 下除 locator 外仍须命名）
FROM read_excel('a.xlsx', sheet='Sheet1', skip=1)
FROM read_csv('a.csv', delim=',')
FROM read_json('a.json', json_path='data')
FROM read_api('https://example.com/x', method='GET', json_path='data')
FROM read_dir('./sales_*.xlsx', sheet='Sheet1')
FROM read_clipboard(delim='\t')
FROM mock_data(10, '用户名:name', '电话:phone')
```

`syntax=2`：查询层只允许 `read()` / `mock_data()` / 已经 LOAD 的表。交付新脚本用 `--strict` 即可，不必强行 `--syntax=2`。

## HTTP

```sql
LOAD orders FROM 'https://api.example.com/query' WITH (
  format='json',
  method='POST',
  body=read_text('payload.json'),
  headers='{"Authorization":"Bearer ${TOKEN}","Content-Type":"application/json"}',
  json_path='data'
);
```

- 超时 30s。HTML 错误页不当 Excel，会报错。
- 未提供 Authorization 且设置了 `SQLXLS_BEARER_TOKEN` 时自动带 Bearer。
- `page_param` / `offset_param`：空页停止。多页不要手写 N 次 LOAD。见 [dynamic.md](dynamic.md)。

JSON 根对象未指定路径时，会尝试 `data` / `items` / `results` / `records` / `rows`，否则取第一个数组。字段取**行 key 并集**。

## mock_data

```sql
LOAD demo FROM mock_data(20, '用户名:name', '电话:phone', '邮箱:email', '公司:company', '城市:city');
```

类型仅：`name` `phone` `email` `company` `city`。未知类型填 `N/A`。

## 多语句

分号分隔。`CREATE TABLE x AS SELECT ...` 与 `LOAD` 一样留在同一内存 Session。最后一条查询是唯一输出。
