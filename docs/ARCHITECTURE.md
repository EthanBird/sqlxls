# sqlxls 技术与功能方案

> 定位：用 SQL 对**任意表格数据**做清理、关联与分析的高性能命令行工具。  
> Excel 只是第一个数据源，不是产品边界。

本文基于当前代码的真实结构与缺陷，给出可落地的目标架构、功能体系和演进路径。第一期实现已经按本文的「连接器 + SQL 改写器 + 批量入库」模型落地，后续引擎可替换为 DuckDB 而不改用户 SQL。

---

## 1. 产品定位

| 问题 | 答案 |
|------|------|
| 给谁用 | 数据清理、报表拼接、临时分析、接口联调、脚本化 ETL |
| 核心承诺 | **一份 SQL，多种来源**：本地文件、剪贴板、HTTP API、目录通配、未来数据库/对象存储 |
| 不是什么 | 不是 Excel 编辑器，不是通用 DBMS，不是 BI 可视化套件 |
| 性能目标 | 百万级行的过滤/聚合在笔记本上可交互；导入不再是逐行提交事务 |

建议对外一句话：

> sqlxls：把 CSV / Excel / JSON / API 当成表，用 SQL 做完再导出。

CLI 名称暂时保持 `sqlxls`，以免破坏已有习惯；架构上按 **SQL Data Tooling** 来设计，而不是「Excel 专用工具」。

---

## 2. 现状与根因

当前实现可以概括为：

```
SQL 字符串
  → 多个正则依次匹配 readexcel / readapi / ...
  → 每个函数自己解析参数、自己 CREATE TABLE、自己逐行 INSERT
  → 用临时表名字符串替换原函数调用
  → SQLite 执行改写后的 SQL
  → 一次性物化结果再打印/导出
```

这套模型能跑通 Demo，但和「通用、高性能、可扩展」的目标冲突。

### 2.1 架构层问题

1. **用正则改写 SQL**  
   `readexcel` 用 `(.+?)` 匹配参数，遇到嵌套括号、JSON、URL 查询串、函数嵌套时会截断或误伤。各函数正则质量不一致（`readapi` 相对完整，`readexcel` / `mock_data` 很脆弱）。
2. **连接器没有统一接口**  
   Excel / JSON / CSV / HTTP 各自建表、各自插数据，类型推断、列名清洗、空表、重复列处理复制粘贴且行为不同。
3. **引擎绑死内存 SQLite**  
   没有「查询引擎」抽象。SQLite 适合嵌入和事务，不是分析型 JOIN / GROUP BY 的最优解；也没有落盘/溢出能力。
4. **没有 Session / Catalog**  
   每次进程只跑一条 SQL。不能 `CREATE TABLE t AS SELECT ...` 后继续查，不能缓存远程结果，不能声明命名数据源。
5. **没有测试**  
   参数解析、Excel skip 行、JSON 结构、HTTP Content-Type 都没有回归，README 与实现已经分叉。

### 2.2 正确性缺陷（已在代码中核实）

| 问题 | 位置 | 影响 |
|------|------|------|
| README 写第三参数 `'str'`，实现第三参数是 `skip_rows`、第四才是 `'str'` | `read_excel.rs` vs README | 用户按文档调用会静默跳过行 |
| `readexcel` 按逗号切参数，路径/Sheet 名带逗号即坏 | `read_excel.rs` | Windows 路径、复杂 Sheet 名 |
| `String::replace` 全文替换函数片段 | `main.rs` | 同一调用出现两次，或子串碰巧相同，会错乱 |
| `readtext` 把原文直接塞进 SQL | `main.rs` + `read_text.rs` | 引号、换行、`--` 注释会破坏 SQL；大文件会撑爆语句 |
| JSON 只看**第一个对象的 key** | `read_json.rs` | 后面行多出来的字段被丢弃 |
| JSON 对象里「随便找一个数组」 | `read_json.rs` | `{meta, data}` 可能吃到错误的数组 |
| `readdir` 用 `CREATE TABLE IF NOT EXISTS` 再 INSERT | `read_excel.rs` | 后续文件多列/少列会插不进去或对错列 |
| HTTP 非 JSON/CSV 一律当 Excel | `read_api.rs` | HTML 错误页被当成 xlsx，报错难懂 |
| HTTP 无超时、无 UA、无分页 | `read_api.rs` | 远程拉数场景不稳定 |
| 空表 `truncate - 2` | 多处建表 SQL | 0 列时 panic |
| Excel 日期存成 `f64` 序列 | `read_excel.rs` | 分析时无法当日期用 |
| 剪贴板、Excel、API 的 CSV 路径三套实现 | 多个文件 | 分隔符、表头、类型行为不一致 |

### 2.3 性能缺陷

1. **逐行 `INSERT` 且无显式事务**（最严重）  
   SQLite 默认一语句一事务。1 万行 Excel ≈ 1 万次 fsync 语义，导入比解析慢一个数量级以上。
2. **每个单元格 `Box<dyn ToSql>`**  
   无意义堆分配，GC 压力大。
3. **全量进内存再查询**  
   calamine 已把 sheet 读进 `Range`；再复制进 SQLite；导出 JSON 又 `Vec` 攒齐。三份数据。
4. **结果集一次性渲染**  
   终端表、JSON 数组都在内存里拼完整对象。
5. **没有文件缓存**  
   同一 xlsx 在脚本里出现两次就解析两次。

---

## 3. 目标架构

```
                    ┌──────────────────────────┐
   SQL / .sql 文件  │  CLI / REPL / -f 函数     │
   命名数据源       └────────────┬─────────────┘
                                 │
                    ┌────────────▼─────────────┐
                    │  Session                 │
                    │  多语句 · 变量 · 目录    │
                    └────────────┬─────────────┘
                                 │
                    ┌────────────▼─────────────┐
                    │  Rewrite / Planner       │
                    │  扫描表函数（非正则）     │
                    │  内层先求值，再替换表名   │
                    └────────────┬─────────────┘
                                 │
              ┌──────────────────┼──────────────────┐
              ▼                  ▼                  ▼
      ┌─────────────┐    ┌─────────────┐    ┌─────────────┐
      │ Connectors  │    │  Ingest     │    │  Engine     │
      │ excel csv   │───▶│  推断 schema│───▶│  SQLite     │
      │ json http   │    │  事务批量写 │    │  (DuckDB 预留)│
      │ glob clip   │    └─────────────┘    └──────┬──────┘
      └─────────────┘                              │
                                            ┌──────▼──────┐
                                            │  Sinks      │
                                            │  table/csv  │
                                            │  json/xlsx  │
                                            │  parquet*   │
                                            └─────────────┘
```

### 3.1 分层职责

| 层 | 职责 | 明确不做什么 |
|----|------|----------------|
| CLI | 参数、输入来源、输出路由、退出码 | 不解析函数参数 |
| Session | 一个内存库、一次运行的表名分配、PRAGMA | 不关心 Excel 细节 |
| Rewrite | 找表函数、嵌套求值、把调用换成表名或 SQL 字面量 | 不用正则匹配括号 |
| Connector | 把外部数据变成「列名 + 行迭代器」 | 不自己拼 INSERT SQL |
| Ingest | 列名清洗、类型推断、事务、批量 INSERT、目录合并 | 不发起 HTTP |
| Engine | 真正执行用户 SQL | 不读文件 |
| Sink | 流式写出 | 不改写 SQL |

第一期引擎仍是 **SQLite in-memory**（编译快、依赖小、SQL 兼容好）。接口上 Session 只依赖 `execute_sql` / `ingest_rows`，第二期可换成 DuckDB。

### 3.2 为什么不立刻换 DuckDB

DuckDB 是这个产品长期正确的分析引擎：向量化、可溢出磁盘、原生 `read_csv` / `read_parquet` / `httpfs`。但：

- `duckdb-rs` 捆绑编译重、体积大，不适合作为「先把模型改对」的第一步；
- 当前瓶颈 80% 在**导入路径**（无事务逐行写），不在 JOIN 算子；
- 用户 SQL 已经按 SQLite 方言写（字符串拼接、部分函数）。

因此：**先把连接器模型和导入性能做对，引擎做成可替换，再引入 DuckDB。** 用户侧表函数语法保持稳定。

---

## 4. SQL 表函数（用户看到的语言）

数据源不是独立子命令，而是 SQL 里的**表函数**。这是本工具相对 `pandas` / `xsv` 的差异化。

### 4.1 统一命名

实现里同时接受下划线与历史别名：

| 推荐 | 别名 | 作用 |
|------|------|------|
| `read_excel(path, sheet?, skip?, str?)` | `readexcel` | 单个工作簿 |
| `read_csv(path, delim?, skip?)` | | CSV / TSV |
| `read_json(path, path?)` | `readjson` | JSON 数组或 `$.a.b` |
| `read_api(url, method?, body?, headers?, json_path?)` | `readapi` | HTTP |
| `read_dir(glob, sheet?, ...)` | `readdir` | 多文件合并 |
| `read_clipboard(delim?, str?)` | `readclipboard` | 剪贴板 |
| `read_text(path_or_url)` | `readtext` | 标量文本（给别的函数当参数） |
| `read(path_or_url, ...)` | | 按扩展名/协议分发 |
| `mock_data(n, 'col:type', ...)` | | 假数据 |

命名参数从第一期开始支持，避免再靠位置参数「第三位到底是 skip 还是 str」：

```sql
SELECT * FROM read_excel('销售.xlsx', sheet='Sheet1', skip=2, str=true)
SELECT * FROM read_json('resp.json', path='data.items')
SELECT * FROM read_api('https://api.example.com/v1/orders',
                       'GET', null,
                       '{"Authorization":"Bearer ${TOKEN}"}',
                       'data')
```

### 4.2 嵌套求值（远程拉数的关键）

Planner **内层先执行**，标量结果作为外层参数，而不是把文件内容拼进 SQL：

```sql
-- URL 放在文件里、或上一步 API 返回
SELECT * FROM read_excel(read_text('path.txt'), 'Sheet1')

-- 动态拼请求体
SELECT * FROM read_api(
  'https://example.com/query',
  'POST',
  read_text('payload.json'),
  '{"Content-Type":"application/json"}'
)
```

`read_text` 的输出是 **Rust 字符串参数**，只有当它出现在最终 SQL 表达式里时才转义成 SQL 字面量。这样既能嵌套，又避免注入和体积爆炸。

### 4.3 多语句 Session

```sql
CREATE TABLE users AS SELECT * FROM read_excel('users.xlsx');
CREATE TABLE orders AS SELECT * FROM read_csv('orders.csv');

SELECT u.name, sum(o.amount) AS total
FROM users u
JOIN orders o ON u.id = o.user_id
GROUP BY u.name
ORDER BY total DESC;
```

最后一条 SELECT 作为输出。中间表留在同一 Session，避免重复解析大文件。

---

## 5. 连接器模型

```rust
pub enum Cell { Null, Int(i64), Real(f64), Text(String), Bool(bool) }

pub struct FrameMeta {
    pub columns: Vec<String>,
}

pub enum FuncOutput {
    Table,              // 已写入 ctx.table_name
    Scalar(String),     // 供嵌套调用或文本输出
}

pub trait TableFunction {
    fn names(&self) -> &[&str];
    fn execute(&self, ctx: &mut ExecCtx, args: &Args) -> Result<FuncOutput>;
}
```

连接器只负责 **I/O + 解码成行**。建表、类型、事务、合并 schema 全部交给 Ingest。

### 5.1 数据源路线图

| 优先级 | 连接器 | 典型场景 | 实现要点 |
|--------|--------|----------|----------|
| P0 | Excel / CSV / JSON | 本地清理 | 统一 Ingest；Excel 日期 → ISO 文本 |
| P0 | HTTP | 远程拉数 | 超时、UA、魔数嗅探、JSON Path、环境变量注入 |
| P0 | glob / clipboard / mock / text | 已有能力修对 | 目录按**列名**对齐，而不是按位置 INSERT |
| P1 | Parquet | 分析中间层 | 可等 DuckDB 原生读 |
| P1 | `read_sql('postgres://...', 'select ...')` | 仓内拉数 | 只用只读查询；连接信息走配置/环境变量 |
| P1 | stdin / 进程管道 | `curl \| sqlxls` | `read('fd://0')` 或 `FROM stdin('csv')` |
| P2 | S3 / OSS | 对象存储 | 复用 HTTP + 签名；或等 DuckDB httpfs |
| P2 | 网页表格 | 复制 HTML | 剪贴板 HTML table → 行 |
| P3 | 写回源文件 | 清理后覆盖 | 见第 8 节，默认**另存**，覆盖要显式开关 |

### 5.2 HTTP 连接器（远程场景）

远程不是「再写一个正则函数」，而是完整的 **Source**：

1. **超时与重试**：默认 30s；5xx / 网络错误有限次重试。  
2. **鉴权**：`headers` JSON；`${ENV}` 展开；`SQLXLS_BEARER_TOKEN` 约定。  
3. **内容协商**：  
   - `application/json` / 魔数 `{` `[` → JSON  
   - `text/csv` / 扩展名 → CSV  
   - ZIP/`PK` → xlsx；OLE 复合文档 → xls  
   - `text/plain` → 标量或单列表  
   - `text/html` → **报错**（附状态码与 body 前 200 字），绝不当 Excel  
4. **JSON Path**：`data`、`data.items`、`$.data.items`，解决 `{code, data:[...]}`。  
5. **分页（P1）**：`page_param` + `page_limit` 或 Link header，合并为同一张表。  
6. **缓存（P1）**：`~/.cache/sqlxls/` 按 URL+header hash，`ttl` 可配。

### 5.3 Schema 合并（目录 / 多文件 / 多页 JSON）

对 `read_dir` / `read_glob`：

1. 第一个文件创建表；  
2. 后续文件按**列名**映射；  
3. 新列 `ALTER TABLE ADD COLUMN`；  
4. 缺列填 NULL；  
5. 可选 `_source` 列记录文件名。

禁止再使用「列数相同就按位置 INSERT」。

JSON 数组：扫描样本行（默认最多 200 行）做 **key 并集**，而不是只信第一行。

---

## 6. 高性能方案

### 6.1 第一期（SQLite，必须做）

| 手段 | 预期效果 |
|------|----------|
| 显式 `BEGIN` + 批量 `INSERT` + `COMMIT` | 导入 10～100× |
| 复用 `Vec<Value>`，去掉 `Box<dyn ToSql>` | 降分配 |
| Session 级 PRAGMA：`synchronous=OFF`、`temp_store=MEMORY`、`cache_size=-64000` | 内存库导入更快 |
| 类型推断后用 INTEGER/REAL/TEXT 建表 | 聚合少做字符串转换 |
| JSON/CSV **流式写出**，不先 `Vec` 全攒 | 大结果集峰值内存下降 |
| 同一 Session 内表函数结果保留 | 多语句不重复 IO |

### 6.2 第二期（引擎升级）

当单机分析成为主路径（多表 JOIN、大 GROUP BY、窗口函数）再上 DuckDB：

- 连接器改为产出 **Arrow RecordBatch** 或直接注册 DuckDB 扫描器；  
- 文件型源优先走 DuckDB 原生 `read_csv_auto` / `read_parquet` / `read_json`；  
- Excel / 剪贴板仍走自研连接器，ingest 到 DuckDB；  
- 超内存时依赖 DuckDB 磁盘溢出，而不是自己写外排。

判断标准：SQLite 优化后，导入已经不是瓶颈，但 `GROUP BY` / `JOIN` 在 5e6+ 行仍明显慢。

### 6.3 第三期（真正大数据）

- 谓词下推：`WHERE date > '2024-01-01'` 在 CSV/Parquet 扫描时过滤；  
- 列裁剪：`SELECT id, amount` 不解码其余列（Parquet 天然适合）；  
- 远程分页/增量：API `updated_at > :cursor`；  
- 本地 catalog：把常用远程表物化成 Parquet 缓存。

Excel 格式本身难以流式谓词下推（xlsx 是 ZIP+XML），超大 xlsx 的策略是：**导入一次，缓存为 Parquet，之后只扫缓存。**

---

## 7. 功能体系

### 7.1 查询与清理（核心）

- 标准 SQL：JOIN、GROUP BY、窗口、CTE、子查询（引擎能力范围内）  
- 数据质量函数（P1，用 SQLite 自定义函数或宏）：`try_int`、`parse_date`、`nullif_blank`、`normalize_phone`  
- `DESCRIBE read_excel('a.xlsx')` / `--schema`：只推断 schema 不跑全查询  
- `--limit` 预览、`--explain` 打印改写后的 SQL 与各连接器耗时

### 7.2 输入

- 命令行 SQL、`.sql` 文件、stdin  
- `-f` 直接跑一个表函数（调试 API / 剪贴板）  
- REPL（P1）：多行编辑、`\dt` 列表、`\schema t`  
- 配置文件 `sqlxls.toml`（P1）：

```toml
[http]
timeout_secs = 30
user_agent = "sqlxls/0.2"

[sources.orders]
url = "https://api.example.com/orders"
headers = { Authorization = "Bearer ${TOKEN}" }
json_path = "data"

[cache]
dir = "~/.cache/sqlxls"
ttl_secs = 3600
```

然后：`SELECT * FROM orders` 由 catalog 解析到 `read_api(...)`。

### 7.3 输出

| 格式 | 现状 | 目标 |
|------|------|------|
| 终端表 | 有，全量渲染 | 自动 `--limit`，宽表截断，TTY 才画表 |
| CSV | 有 | 流式；可设分隔符 |
| JSON | 全量 pretty | 流式数组；`--ndjson` |
| xlsx | 有 | 类型保留（数字不要变文本） |
| Parquet | 无 | P1，作为与 DuckDB/Spark 交接格式 |
| 剪贴板 | 无写出 | P1 ` -o clipboard` |
| 退出码 | 几乎总是 0 | 查询失败 ≠ 0；`--fail-on-empty` |

### 7.4 可观测与安全

- 每个连接器日志：路径、行数、耗时、推断类型（`--verbose`）  
- HTTP 默认不打印 header 里的 token  
- 本地路径不做任意限制（本机工具），但 `read_api` 应有 `--allow-http` 以外的 **SSRF 提示**（文档说明：请求由用户本机发出）  
- 不把远程 body 当 Excel，除非魔数匹配

---

## 8. 写回与「用 SQL 改 Excel」

TODO 里的 `UPDATE`/`INSERT` 写回源文件，建议拆成三档，避免一上来做「就地改 xlsx 并保留格式」这种极难做对的事：

1. **另存结果**（已有 `-o`）：默认、安全。  
2. **物化中间表再导出**：`CREATE TABLE cleaned AS SELECT ...;  --output cleaned.xlsx`  
3. **就地写回（P2）**：仅 CSV 先做；xlsx 只覆盖指定 sheet 的值区，格式/合并单元格/公式明确 **不保证**。需要 `--in-place` 二次确认。

分析工具的主路径是 **读 → SQL → 新文件**，不是 Excel 宏替代品。

---

## 9. 工程与质量

- **库与 CLI 分离**：`sqlxls` crate 暴露 `Session`，`main` 只做参数。所有行为用 `cargo test` 覆盖。  
- **黄金测试**：小型 xlsx/csv/json fixture；表函数嵌套；HTTP 用 `httpmock` 或本地文件 URL。  
- **CI**：Linux / macOS / Windows 跑测试；Release 预编译三个平台（README 原 TODO）。  
- **错误信息**：带函数名、参数、文件路径、SQL 改写结果（debug）。禁止 `truncate` 空串 panic。  
- **版本策略**：表函数语法 0.x 允许小调整；1.0 起别名长期保留。

---

## 10. 演进路径（按技术依赖，不是按日历）

### 第一期（本次落地）— 把模型改对

- 括号感知的表函数扫描器，替换正则改写  
- 统一 `Args` / `Cell` / `ingest_rows`  
- 事务批量导入 + PRAGMA  
- `read_csv`、`read()` 分发、JSON Path、HTTP 超时与内容嗅探  
- JSON key 并集、目录按列名合并  
- Excel 日期、skip/str 与文档对齐、命名参数  
- 集成测试 + 架构文档

### 第二期 — 远程与分析

- `sqlxls.toml` catalog、结果缓存  
- HTTP 分页、重试、`${ENV}`  
- REPL、`--explain` / `--schema`  
- Parquet 输出；评估 DuckDB feature flag  
- `read_sql`（Postgres/MySQL 只读）

### 第三期 — 平台化

- DuckDB 默认引擎、Arrow 交换  
- 对象存储、谓词下推  
- 受控的 CSV/xlsx 写回  
- 可选：把 `Session` 嵌进 Python（PyO3）给 notebook 用，SQL 仍是唯一语言表面

---

## 11. 明确不做

- 不实现完整 Excel 公式引擎。  
- 不把工具做成「可视化 ETL 画布」。  
- 第一期不绑定云账号、不在服务端代跑 SQL（保持本地 CLI）。  
- 不对用户 SQL 做权限模型；信任运行它的人。

---

## 12. 成功标准

1. 同一条 SQL 能 JOIN：Excel + CSV + HTTP JSON。  
2. 10 万行 CSV 导入明显快于「无事务逐行 INSERT」的旧实现。  
3. README 里的每一个例子都有测试。  
4. 新增一种数据源只需要：一个 `TableFunction` + 解码器，不必改 `main` 和 Ingest。  
5. 换引擎时，用户 SQL 与表函数名字保持不变。
