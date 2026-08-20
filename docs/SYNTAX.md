# sqlxls 语法设计

> 结论先说：**查询必须是标准 SQL；数据源用一套封闭、可版本化的小语言。**  
> 两者不要再混成「看起来像 SQL、其实是字符串改写」的方言。

本文是语法层的稳定化方案。实现层（连接器、Ingest、引擎）见 [ARCHITECTURE.md](./ARCHITECTURE.md)。

---

## 1. 现在为什么不稳

当前用户看到的是：

```sql
SELECT * FROM read_excel('a.xlsx', 'Sheet1', 2, 'str')
```

这不是 SQLite 能理解的 SQL。工具在执行前用扫描器找到 `read_excel(...)`，物化成临时表，再把调用替换成表名。问题不在「能不能跑」，而在**语言契约会一直漂**：

| 不稳定点 | 后果 |
|----------|------|
| 位置参数超载 | 第三位既可能是 `skip` 也可能是 `'str'`，文档一改，旧脚本静默错 |
| 每个源一个函数、参数布局不同 | `read_api` 五段位置参数，`read_excel` 四段，记不住、难演进 |
| 表函数可以出现在任意位置 | `SELECT read_text('a')`、`WHERE x = read_csv('b')` 语义含糊 |
| 专用函数名膨胀 | `readexcel` / `read_excel` / `read()` 三套，1.0 很难删 |
| 查询方言绑死引擎 | 在 FROM 里堆自定义函数，换 DuckDB 时还要再做一层改写 |

「多加几个表函数」会让表面更丰富，语法只会更脆。稳定要从**分层**来，不是从函数清单来。

---

## 2. 稳定原则

1. **两层语言，互不侵入**  
   - **Source 语言**：只负责「把外部数据变成表」。封闭、命名参数、可版本化。  
   - **Query 语言**：物化之后的 `SELECT` / `WITH` / `JOIN` 是引擎 SQL。不在查询里发明循环、表函数或宏。允许少量确定性标量（日期解析），失败返回 NULL。
2. **一个构造器**  
   规范形式只有 `read(locator, 命名选项…)`。`read_excel` 等是语法糖，desugar 到 `read`。
3. **第一个参数永远是定位符，其余一律命名**  
   禁止再增加位置参数。`sheet` / `skip` / `json_path` 只能写成 `sheet='Sheet1'`。
4. **定位符是 URI，不是「有时是路径有时是 URL」**  
   `file`、`https`、`glob`、`clip` 用同一套 locator，用 `format=` 覆盖推断。
5. **解析，不扫描替换当语言**  
   Source 表达式有独立文法。查询侧只允许表函数出现在 **表位置**（`FROM` / `JOIN`），最终应交给 SQL 解析器做 TableFactor，而不是全文搜标识符。
6. **语言版本显式**  
   `--syntax=1`（默认）。破坏性变更只进 `--syntax=2`，旧脚本不默默变语义。

---

## 3. 规范语法（syntax=1）

脚本是语句序列。语句三类：`SET`、`LOAD`、查询 SQL。

```ebnf
script          = { statement ";" } ;

statement       = set_stmt
                | load_stmt
                | query_stmt ;          (* 标准 SQL，由引擎解析 *)

set_stmt        = "SET" ident "=" value ;

load_stmt       = "LOAD" [ "TABLE" ] ident "FROM" source { for_clause } ;

source          = locator [ "WITH" options ]
                | "EACH" each_spec [ "WITH" options ]
                | read_call ;

each_spec       = "(" locator { "," locator } ")"
                | "GLOB" locator ;

for_clause      = "FOR" for_vars "IN" for_domain ;

for_vars        = ident { "," ident }
                | "(" ident { "," ident } ")" ;

for_domain      = "(" value { "," value } ")"
                | integer ".." integer [ "STEP" integer ]
                | "DATE" range_bound ".." range_bound [ "STEP" date_step ]
                | "GLOB" locator ;

range_bound     = integer | string ;    (* 日：2024-01-15 / 20240115；月：2024-01 / 202401 *)

date_step       = integer | "MONTH" | integer "MONTH" | integer "DAY" ;

locator         = string ;              (* URI 或路径 *)

options         = "(" named_arg { "," named_arg } ")" ;

read_call       = read_name "(" locator { "," named_arg } ")" ;

read_name       = "read" | sugar_name ;

sugar_name      = "read_excel" | "read_csv" | "read_json"
                | "read_api" | "read_dir" | "read_clipboard"
                | "read_text" | "mock_data"
                | (* 历史别名，见 §6 *) ;

named_arg       = ident "=" value ;

value           = string | number | boolean | "null" | read_call ;

ident           = letter { letter | digit | "_" } ;

(* 查询层：不发明循环/表函数。过渡期允许 table_factor 上的 read_call，见 §5。
   日期标量 parse_date / parse_datetime / from_unix / to_unix / excel_serial 由引擎注册。 *)
```

动态展开（相似 URL、目录、分页、多 Sheet）见 [DYNAMIC.md](./DYNAMIC.md)。默认扇入为一张表 + `_source` / `_region` / `_page` / `_sheet`，查询仍是标准 SQL。

`LOAD` 的 `WITH` **只接受命名参数**。没有第三位到底是 skip 还是 str 这种事。

### 3.1 推荐写法（脚本）

```sql
LOAD users  FROM 'users.xlsx' WITH (format='excel', sheet='Sheet1', skip=2);
LOAD orders FROM 'https://api.example.com/orders' WITH (format='json', json_path='data');
LOAD extra  FROM 'dept.csv';   -- format 由扩展名推断

SELECT u.name, SUM(o.amount) AS total
FROM users u
JOIN orders o ON u.id = o.user_id
GROUP BY u.name
ORDER BY total DESC;
```

`SELECT` 里没有任何 `read_*`。这是语法稳定的主路径：  
**先绑定表，再写普通 SQL。**

### 3.2 一行探索（糖）

交互时仍允许把 `read(...)` 写在表位置：

```sql
SELECT name, SUM(amount)
FROM read('sales.csv')
GROUP BY name;
```

这是 `LOAD` 的匿名形式，desugar 为「物化到临时表 → 标准 SQL」。  
`--strict` 下，除 locator 外必须命名：

```sql
-- 合法
FROM read('a.xlsx', format='excel', sheet='Sheet1', skip=2)

-- 非法（strict）
FROM read_excel('a.xlsx', 'Sheet1', 2, 'str')
```

### 3.3 定位符与 format

| locator | 默认 format |
|---------|-------------|
| `*.xlsx` / `*.xls` | `excel` |
| `*.csv` / `*.tsv` | `csv` |
| `*.json` | `json` |
| `http://` `https://` | `json`（仍按 Content-Type / 魔数纠正） |
| `glob:sales_*.csv` 或含 `*` `?` | `glob` |
| `clip:` / `clipboard:` | `clipboard` |

显式 `format=` 覆盖推断。新增数据源 = 新的 `format` 枚举值 + 一组命名选项，**不加新的顶层关键字**。

规范选项（syntax=1，按 format 关闭集合）：

| format | 允许的选项 |
|--------|------------|
| `excel` | `sheet`, `skip`, `str`, `header` / `has_header`, `columns` / `names` / `colnames` |
| `csv` | `delim` / `sep`, `skip`, `str`, `encoding` / `charset`, `header` / `has_header`, `columns` / `names` / `colnames` |
| `json` | `json_path` / `path`, `encoding` / `charset` |
| `http` | `method`, `body` / `payload`, `headers`, `json_path`, `encoding`, 分页选项；CSV/Excel 体同样允许 `header`、`columns` |
| `glob` | 同上，外加按文件扩展名分发；Excel/CSV 允许 `header`、`columns` |
| `clipboard` | `delim`, `str`, `encoding`；Excel/CSV 允许 `header`、`columns` |
| `text` | `encoding`（标量，只能出现在 `read()` 参数里，不能单独当表） |

未列出的选项：syntax=1 **报错**，不要忽略。忽略等于以后无法再使用这个名字。

Excel / CSV：默认第一行是列名。`header=false` 把第一行当数据（自动名 `col_N`）。`columns='id,name'` 只改列名，不隐含无表头；无表头自定义名要写成 `header=false, columns='…'`。

传输层（任意 `http(s)` 定位符，与 `format=` 正交）：`insecure` / `verify` / `ssl_verify` / `tls_verify`。默认校验证书；`insecure=true` 或 `verify=false` 跳过（`curl -k`）。

---

## 4. 为什么不选其他表面

| 方案 | 优点 | 不采用的原因 |
|------|------|----------------|
| 继续堆 `read_excel` / `read_mysql` / `read_s3` | 短 | 参数布局无法收敛，1.0 无法冻结 |
| 发明一套全新查询语言 | 可控 | 用户要的是 SQL；学习成本和引擎生态都亏 |
| 只用 CLI：`sqlxls load a.xlsx --as t` | SQL 最纯 | 多源 JOIN、脚本化变差；Source 语言仍得存在，只是挪到 argv |
| DuckDB 式 `FROM 'a.csv'` | 短 | 选项没地方放；远程 API / Excel sheet 仍要第二通道 |
| SQLite `CREATE VIRTUAL TABLE ... USING csv(...)` | 标准 | 每个源写一堆 DDL，和「一条 SQL 探数据」的产品相反 |

`LOAD ... FROM locator WITH (...)` 加上可选的 `FROM read(...)` 糖，是「脚本稳定」和「一行探索」之间的平衡。

---

## 5. 解析策略（比全文扫描稳）

目标管线：

```
脚本
 ├─ 按分号切语句（忽略字符串/注释）
 ├─ 以 LOAD 开头 → Source 文法
 └─ 其余 → SQL 解析器（TableFactor 上的 read_call 才合法）
         → 物化
         → 引擎执行纯 SQL
```

约束：

- `read_text` / `read(..., format='text')` 是**标量**，只允许出现在另一个 Source 的参数里，不允许 `FROM read_text('a.csv')`。
- 查询列表、`WHERE`、`SELECT` 表达式里出现 `read_*`：syntax=1 过渡期可警告，syntax=2 **非法**。
- 字符串与注释中的 `read_csv(...)` 不是源，解析器不得触摸。

全文标识符扫描是过渡实现，不是语言定义。语言定义以本文文法为准。

---

## 6. 兼容政策

| 写法 | syntax=1 默认 | `--strict` / 未来 syntax=2 |
|------|----------------|------------------------------|
| `LOAD t FROM 'a.csv'` | 推荐 | 推荐 |
| `FROM read('a.csv')` | 允许 | 允许 |
| `FROM read_excel('a.xlsx', sheet='S')` | 允许（糖） | 允许 |
| `FROM readexcel(...)` 等历史别名 | 允许 | 警告 |
| `FROM read_excel('a.xlsx', 'S', 2)` | 允许（兼容） | **错误** |
| `readtext()` 把原文拼进 SQL | **已废除** | 已废除 |
| 查询表达式里的表函数 | 允许但不要用 | **错误** |

别名只做 desugar，不再给别名增加新参数。新能力只加在 `read(..., format=..., 命名选项)` 和 `LOAD ... WITH`。

---

## 7. 版本怎么升

- **syntax=1**：本文。`LOAD` + 规范 `read` + 旧表函数糖 + 位置参数兼容。  
- **syntax=2**（在旧写法使用率降下来之后）：删除位置参数和查询层表函数；`FROM` 只接受表名或规范 `read()`；建议默认 `--strict`。  
- 不在同一 syntax 版本里改变已有命名选项的含义。要变就改名（`skip` 不改成「有时是 header row」）。

CLI：

```bash
sqlxls script.sql                  # syntax=1
sqlxls script.sql --strict         # 位置参数视为错误
sqlxls script.sql --syntax=1
```

---

## 8. 和引擎替换的关系

Source 层求值结果是一张普通表（或未来的 Arrow 扫描器）。  
Query 层因此可以是 SQLite，也可以是 DuckDB，**不必带着 `read_excel` 一起搬家**。

这是语法稳定真正要买到的东西：  
**用户脚本的查询部分是标准 SQL，数据源部分是一份短文法。** 两者各自进化，互不绑架。

---

## 9. 当前落地（对照本文）

| 规范 | 状态 |
|------|------|
| `LOAD … FROM locator WITH (命名选项)` | 已实现 |
| `SET` / `--set` / `${var}` | 已实现 |
| `FOR` / `EACH` 展开后 UNION | 已实现 |
| HTTP 分页 / glob `_source` / `sheet='*'` | 已实现 |
| 规范 `read(locator, format=…, 命名选项)` | 已实现 |
| 糖函数 desugar 到 `read(..., format=…)` | 已实现 |
| `--syntax=1` / `--syntax=2` / `--strict` | 已实现 |
| 未列出的选项报错（关闭选项集） | 已实现 |
| `read_text` 不能当表 | 已实现 |
| 表函数只能出现在 FROM/JOIN（SQL 解析器判定） | 已实现 |
| SELECT/WHERE 里的表函数报错；标量表达式仅 syntax=1 非 strict 警告 | 已实现 |
| syntax=2：查询层只允许 `read()` / `mock_data()` | 已实现 |
