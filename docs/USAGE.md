# sqlxls 使用手册（v0.3）

一份查询，多种表格来源。把 Excel / CSV / JSON / HTTP / 目录 / 剪贴板当成表，用 **标准 SQL** 清洗、关联、聚合，再导出。

分场景教学见 [tutorials/](tutorials/README.md)。语法规范见 [SYNTAX.md](SYNTAX.md)。

---

## 1. 安装

从 [Releases](https://github.com/EthanBird/sqlxls/releases) 下载对应文件，改名 `sqlxls`（Windows 为 `.exe`）放进 `PATH`：

| 平台 | 文件 |
|------|------|
| Linux x86_64 | `sqlxls-x86_64-unknown-linux-gnu` |
| macOS Apple Silicon | `sqlxls-aarch64-apple-darwin` |
| macOS Intel | `sqlxls-x86_64-apple-darwin` |
| Windows x86_64 | `sqlxls-x86_64-pc-windows-msvc.exe` |

源码编译需要 Rust 1.88+：

```bash
cargo build --release
# 可执行文件：target/release/sqlxls
```

```bash
sqlxls --help
sqlxls --version
```

---

## 2. 命令行

```bash
sqlxls "SQL 或脚本路径" [选项]
sqlxls -f "read('data.csv')"          # 只跑一个表函数
```

| 选项 | 含义 |
|------|------|
| `-o` / `--output` | 导出路径。扩展名决定格式：`xlsx` / `csv` / `json` / `ndjson` |
| `--strict` | 除定位符外必须命名参数。新脚本请打开 |
| `--syntax=1\|2` | `1` 默认，允许 `read_excel` 等糖；`2` 查询层只允许 `read()` / `LOAD` |
| `--set name=value` | 绑定变量，可重复。给脚本里的 `${name}` 用 |
| `--explain` | 在 stderr 打印改写后的 SQL / 展开计划，再执行 |
| `-f` / `--func` | 直接执行一个表函数，不必包 `SELECT * FROM` |

只有**最后一条** `SELECT` / `WITH` / `PRAGMA` 会打印或写入 `-o`。前面的 `LOAD`、`CREATE TABLE` 只改本次进程的内存库。

```bash
sqlxls analysis.sql --strict --set region=east -o result.xlsx
sqlxls analysis.sql --explain
```

---

## 3. 两层语言（必须分开）

```
Bind    SET x = '...'     --set x=...      环境变量
Source  LOAD 表 FROM ...  FOR / EACH        把外部数据变成表
Query   SELECT / JOIN / GROUP BY            引擎标准 SQL（SQLite）
```

推荐脚本：

```sql
LOAD users  FROM 'users.xlsx' WITH (format='excel', sheet='Sheet1');
LOAD orders FROM 'orders.csv';

SELECT u.name, SUM(o.amount) AS total
FROM users u
JOIN orders o ON u.id = o.user_id
GROUP BY u.name
ORDER BY total DESC;
```

- `LOAD` 的**表名**只能是 ASCII：`sales`，不要用 `销售`。
- **列名**可以是中文，SQL 里用双引号：`"销售额"`。
- 字符串用单引号。查询里**不要**写 `${var}`（那是定位符/选项用的）。
- 未知 `WITH` 选项会报错，不会被忽略。

一行探索可以 `FROM read('sales.csv')`。交付仍用 `LOAD` + `--strict`。

---

## 4. 定位符与 format

`http(s)` **始终走 HTTP 传输**；`format=` 只决定怎么解码。

| 定位符 | 默认 format |
|--------|-------------|
| `*.xlsx` `*.xls` `*.xlsm` | `excel` |
| `*.csv` `*.tsv` | `csv` |
| `*.json` | `json` |
| `http://` `https://` | `http`（再按 Content-Type / 魔数纠正） |
| 路径含 `*` `?` 或 `glob:` | `glob` |
| `clip:` `clipboard:` | `clipboard` |

推断不出时必须写 `format=`。

### 各 format 选项（关闭集合）

| format | 选项 |
|--------|------|
| `excel` | `sheet`, `skip`, `str`, `header`/`has_header`, `columns`/`names`/`colnames` |
| `csv` | `delim`/`sep`, `skip`, `str`, `encoding`/`charset`, `header`/`has_header`, `columns`/`names`/`colnames` |
| `json` | `json_path`/`path`, `encoding`/`charset` |
| `http` | `method`, `body`/`payload`, `headers`, `json_path`, `encoding`, `sheet`, `skip`, `delim`, `str`，以及分页选项；解码 CSV/Excel 时同样支持 `header`、`columns` |
| `glob` | 与被匹配文件相同，外加 `encoding`；Excel/CSV 同样支持 `header`、`columns` |
| `clipboard` | `delim`, `str`, `encoding`；Excel/CSV 同样支持 `header`、`columns` |
| `text` | `encoding`（**标量**，不能 `FROM read_text(...)`） |

分页（HTTP）：`page_param`, `page_from`, `page_to`, `page_size`, `page_size_param`, `offset_param`, `offset_step`, `stop`。  
通用：`include_source`（默认带来源列；`false` 关掉）。  
HTTPS 证书：默认校验。内网自签 / 公司代理导致失败时，加 `insecure=true`（或 `verify=false`），相当于 `curl -k`。只用于你信任的地址。

Excel / CSV 表头：

- 默认第一行是列名。
- 没有表头：`header=false`（列名为 `col_0`, `col_1`, …）。
- 自定义列名：`columns='id,name,金额'`。只改名、不改变「第一行是不是表头」。
- 无表头又要自己起名：必须 **两个都写** `header=false, columns='…'`。只写 `columns` 仍会把第一行当表头丢掉。
- `has_header` ≡ `header`；`names` / `colnames` ≡ `columns`。`columns` 也可以是 JSON 数组字符串 `["id","name"]`。

---

## 5. 动态源速查

相似文件 / URL **不要复制 N 段 SQL**。展开后默认按列名 UNION 成一张表。

```sql
SET base = 'https://api.example.com';

LOAD orders FROM '${base}/${region}/orders' WITH (format='json', json_path='data')
FOR region IN ('east', 'west');

LOAD sales FROM EACH GLOB './sales_*.csv';

LOAD items FROM 'https://api.example.com/items' WITH (
  format='json', json_path='data', page_param='page', page_to=50
);

LOAD book FROM 'workbook.xlsx' WITH (format='excel', sheet='*');

LOAD daily FROM '${base}/day/${d}' WITH (format='json')
FOR d IN DATE '2024-01-01'..'2024-01-31';
```

| 写法 | 含义 |
|------|------|
| `SET x = 'a'` / `--set x=a` | 绑定；定位符写 `${x}` |
| `FOR x IN ('a','b')` | 列表 |
| `FOR (a,b) IN (('x','1'),('y','2'))` | 成对 zip，不是笛卡尔 |
| `FOR x IN (...) FOR y IN (...)` | 嵌套，所有组合 |
| `FOR n IN 1..12 STEP 2` | 整数闭区间 |
| `FOR d IN DATE '2024-01-01'..'2024-01-31'` | 日期；**必须**有 `DATE` |
| `FOR ym IN DATE '2024-01'..'2024-12'` | 月份 |
| `FOR d IN DATE '…'..'…' STEP 7` / `STEP MONTH` | 步长 |
| `FROM EACH ('a.csv','b.csv')` | 一组定位符 |
| `FROM EACH GLOB './x_*.csv'` | 通配 |

来源列：`FOR region` → `_region`；文件/URL → `_source`；页 → `_page`；sheet → `_sheet`。

详情：[DYNAMIC.md](DYNAMIC.md)、[教程：相似源](tutorials/07-dynamic-sources.md)。

---

## 6. 编码与远程二进制

- 文本（CSV/JSON）**默认 UTF-8**（含 BOM）。
- GBK / GB18030 / GB2312 / Big5：`encoding='gbk'`，或 HTTP `Content-Type: text/csv; charset=gbk`。
- 解析失败会提示写 `encoding='gbk'`，不会静默乱码。
- 远程 Excel 看文件魔数（xlsx=`PK\x03\x04`，xls=OLE），即使 `Content-Type` 是 `application/octet-stream`。不要给 Excel 设 `encoding`。
- HTML 错误页会报错，不会当成表格。

---

## 7. 查询层（SQLite）

物化之后按 SQLite 写。中文列必须 `"列名"`。

日期转换（失败为 `NULL`）：

| 函数 | 作用 |
|------|------|
| `parse_date(x)` | ISO、`YYYYMMDD`；日>12 的 `15/01/2024` 可自动 |
| `parse_date(x,'dmy'\|'mdy'\|'ymd')` | 明确日/月顺序；也可用 `'%Y年%m月%d日'` |
| `parse_datetime(x)` | RFC3339、空格分隔时间 |
| `from_unix(ts)` | Unix 秒；`\|x\| ≥ 1e12` 当毫秒 |
| `to_unix(ts)` | 回到秒 |
| `excel_serial(n)` | Excel 序列 → ISO（与导入相同） |
| `date` / `strftime` / `unixepoch` | SQLite 自带 |

`01/02/2024` 这种日月都 ≤12 的，必须写 `'dmy'` 或 `'mdy'`。

Excel 导入的日期已经是 ISO 文本，可以直接 `'2024-01-01'` 比较。

---

## 8. 鉴权

```bash
export SQLXLS_BEARER_TOKEN='...'   # 自动带 Authorization: Bearer
export TOKEN='...'                 # 供 headers 里 ${TOKEN}
```

```sql
headers='{"Authorization":"Bearer ${TOKEN}","Content-Type":"application/json"}'
```

不要把密钥写进要提交的 `.sql`。

---

## 9. 现在做不到

- 就地改 xlsx / 写回原工作簿
- 直连 Postgres / MySQL / S3 / Parquet
- HTTP cursor / Link 分页（页码/offset 可以）
- `sqlxls.toml` catalog、REPL、独立 `--schema` 旗标（用 `PRAGMA table_info`）
- 亿级仓内分析（内存 SQLite，适合笔记本可交互规模）

---

## 10. 最小可跑示例

```sql
-- hello.sql
LOAD t FROM 'data.csv';
SELECT * FROM t LIMIT 8;
```

```bash
sqlxls hello.sql --strict
sqlxls hello.sql --strict -o out.xlsx
```
