# sqlxls

**sqlxls** 是一个用 SQL 做数据清理与分析的命令行工具。Excel 只是数据源之一：本地 CSV / JSON、目录通配、剪贴板、HTTP API 都可以当成表来 JOIN、过滤、聚合，再导出成表格文件。

底层目前是内存 SQLite；导入路径使用**显式事务 + 批量 INSERT**。  
技术架构见 [`docs/ARCHITECTURE.md`](docs/ARCHITECTURE.md)，**语法稳定方案**见 [`docs/SYNTAX.md`](docs/SYNTAX.md)。

## 能做什么

- **推荐**：`LOAD 表 FROM '文件' WITH (...)` 绑定数据源，后面写标准 SQL
- 一行探索：把 `read(...)` 写在 `FROM` 里（糖）
- 嵌套调用：`read_csv(read_text('path.txt'))`，内层先求值，不会把文件内容拼进 SQL
- `--strict`：除定位符外必须用命名参数；`--syntax=2` 查询层只允许 `read()` / `LOAD`
- 导出终端表 / CSV / JSON / NDJSON / xlsx

## 安装

需要较新的稳定版 Rust（建议 1.88+）：

```bash
cargo build --release
```

可执行文件在 `target/release/sqlxls`。

## 用法

```bash
sqlxls "SQL 语句" [-o 输出文件]
sqlxls query.sql [-o 输出文件]
sqlxls -f "read_api('https://example.com/data.json')"
sqlxls "SELECT ..." --explain    # 打印改写后的 SQL
sqlxls script.sql --strict      # 禁止位置参数超载
sqlxls script.sql --syntax=2    # 查询层只允许 read() / LOAD
```

### 0. 推荐：先 LOAD，再写标准 SQL

```sql
LOAD users  FROM 'users.xlsx' WITH (format='excel', sheet='Sheet1');
LOAD orders FROM 'https://api.example.com/orders' WITH (format='json', json_path='data');

SELECT u.name, SUM(o.amount) AS total
FROM users u
JOIN orders o ON u.id = o.user_id
GROUP BY u.name;
```

查询部分不再出现 `read_*`。这是长期要冻结的写法。

整段输入如果就是一个表函数，可以省略 `SELECT * FROM`：

```bash
sqlxls "read_csv('sales.csv')"
```

### 1. Excel

```bash
sqlxls "SELECT * FROM read_excel('data.xlsx', 'Sheet1') WHERE id > 100"
sqlxls "SELECT * FROM read_excel('data.xlsx')"                      # 默认第一个 sheet
sqlxls "SELECT * FROM read_excel('data.xlsx', sheet='Sheet1', skip=2, str=true)"
```

第三参数既可以是跳过行数，也可以是 `'str'`（与早期文档兼容）：

```bash
sqlxls "SELECT * FROM read_excel('data.xlsx', 'Sheet1', 'str')"
```

### 2. CSV / 自动识别

```bash
sqlxls "SELECT category, SUM(price) FROM read_csv('sales.csv') GROUP BY category"
sqlxls "SELECT * FROM read('orders.tsv')"          # 按扩展名分发
sqlxls "SELECT * FROM read('https://example.com/data.json')"
```

### 3. JSON（支持路径，字段取并集）

```bash
sqlxls "SELECT * FROM read_json('resp.json')"
sqlxls "SELECT * FROM read_json('resp.json', json_path='data.items')"
```

根对象若未指定路径，会依次尝试 `data` / `items` / `results` / `records` / `rows`，否则取第一个数组。

### 4. HTTP API

```bash
sqlxls "SELECT * FROM read_api('https://api.example.com/orders')"
sqlxls "SELECT * FROM read_api(
  'https://api.example.com/query',
  'POST',
  read_text('payload.json'),
  '{\"Authorization\":\"Bearer \${TOKEN}\"}',
  'data'
)"
```

参数：`url, method?, body?, headers?, json_path?`。`headers` 为 JSON 对象；`${ENV}` 会展开。若设置了环境变量 `SQLXLS_BEARER_TOKEN` 且未提供 Authorization，会自动带上 Bearer。默认 30s 超时。HTML 错误页**不会**再被当成 Excel。

### 5. 目录合并（按列名对齐）

```bash
sqlxls "SELECT * FROM read_dir('./sales_*.xlsx', 'Sheet1')"
sqlxls "SELECT * FROM read_dir('./logs_*.csv')"
```

后续文件多出来的列会 `ALTER TABLE` 补上，缺的列填 NULL。不要再假设「列数相同就按位置插入」。

### 6. 剪贴板与假数据

```bash
sqlxls "SELECT * FROM read_clipboard() LIMIT 10"
sqlxls "SELECT * FROM read_clipboard(delim='\t')"
sqlxls "SELECT * FROM mock_data(10, '用户名:name', '联系方式:phone', '所在城市:city')"
```

`mock_data` 类型：`name` / `phone` / `email` / `company` / `city`。

### 7. 多表 JOIN 与多语句

```bash
sqlxls "
SELECT a.id, a.name, b.department
FROM read_excel('users.xlsx') AS a
JOIN read_csv('dept.csv') AS b ON a.id = b.user_id
"
```

```sql
-- save as report.sql
CREATE TABLE users AS SELECT * FROM read_excel('users.xlsx');
CREATE TABLE orders AS SELECT * FROM read_csv('orders.csv');
SELECT u.name, SUM(o.amount) AS total
FROM users u
JOIN orders o ON u.id = o.user_id
GROUP BY u.name
ORDER BY total DESC;
```

```bash
sqlxls report.sql -o result.xlsx
```

### 8. 导出

```bash
sqlxls "SELECT * FROM read_csv('data.csv')" -o result.xlsx
sqlxls "SELECT * FROM read_csv('data.csv')" -o result.csv
sqlxls "SELECT * FROM read_csv('data.csv')" -o result.json
sqlxls "SELECT * FROM read_csv('data.csv')" -o result.ndjson
```

## 表函数一览

| 函数 | 别名 | 说明 |
|------|------|------|
| `read_excel(path, sheet?, skip?, str?)` | `readexcel` | 工作簿 |
| `read_csv(path, delim?, skip?)` | `readcsv` | CSV / TSV |
| `read_json(path, json_path?)` | `readjson` | JSON |
| `read_api(url, method?, body?, headers?, json_path?)` | `readapi` | HTTP |
| `read_dir(glob, sheet?, ...)` | `readdir` / `read_glob` | 多文件合并 |
| `read_clipboard(delim?, str?)` | `readclipboard` | 剪贴板 |
| `read_text(path_or_url)` | `readtext` | 标量文本，供嵌套参数 |
| `read(path_or_url, ...)` | | 按扩展名 / `http(s)` 分发 |
| `mock_data(n, 'col:type', ...)` | | 假数据 |

命名参数示例：`read_excel('a.xlsx', sheet='S1', skip=1, str=true)`。

## 设计原则（摘要）

1. **连接器只负责解码成行**；建表、类型推断、事务写入由统一 Ingest 完成。  
2. **表函数用括号扫描器解析**，不再靠正则截参数。  
3. **引擎可替换**：第一期 SQLite，分析变重后再上 DuckDB，用户 SQL 尽量不变。  

细节、已知缺陷对照、以及远程分页 / Catalog / 写回等后续能力见 [架构方案](docs/ARCHITECTURE.md)。

## 路线图

- [x] 统一连接器 + 事务导入 + 表函数扫描器
- [x] CSV / JSON Path / HTTP 超时与内容嗅探
- [ ] `sqlxls.toml` 命名数据源与结果缓存
- [ ] HTTP 分页、REPL、`--schema`
- [ ] Parquet 输出；可选 DuckDB 引擎
- [ ] `read_sql`（Postgres / MySQL 只读）
- [ ] GitHub Actions 预编译 Windows / macOS / Linux 二进制
