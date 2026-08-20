# 动态源：一份查询，扇出到多个相近数据源

> 查询层仍然是标准 SQL。动态性全部放在 **Bind + Source 展开**，展开结果默认 **按列名 UNION 成一张表**，并带来源维度列。分析 SQL 只写一次。

本文回答的问题是：相似但不相同的文件、URL、分页、Sheet、日期、区域……要怎样表达，才不用复制一堆 `LOAD` 和 `SELECT`。

实现层见 [ARCHITECTURE.md](./ARCHITECTURE.md)，基础文法见 [SYNTAX.md](./SYNTAX.md)。

---

## 1. 问题的真实形状

用户遇到的不是「再加三个表函数」，而是同一类作业在 **N 个轴** 上变化：

| 变化轴 | 例子 | 错误做法 | 正确扇出 |
|--------|------|----------|----------|
| 定位符 | 换文件名 / 换 URL 路径 | 复制整段 SQL | `${var}` + `SET` / `--set` |
| 同质文件集 | 目录下 12 个月 CSV | 12 次 LOAD + 12 次 SELECT | glob / `EACH` → 一张表 |
| HTTP 分页 | `?page=1..n` | 手写 N 个 URL | `page_param` 直到空页 |
| Excel 多表 | 每个 sheet 结构相同 | 每个 sheet 一个 LOAD | `sheet='*'` |
| 区域 / 租户 | `/east/orders` vs `/west/orders` | 每个区域一份脚本 | `FOR region IN (...)` |
| 日期窗口 | `dt=2024-01-01` | 日历展开手写 | `FOR day IN (...)` 或范围 |
| 混合相近源 | 两个不同 host、同一 JSON 形 | 两段几乎一样的 LOAD | `EACH ('url1','url2')` |

共同点：**物理源是多个，逻辑表是一个。**  
痛点出在「每个源都要命名一张表，查询跟着复制」。所以默认策略是 **扇出 → 扇入（UNION BY NAME）→ 一张表 + 来源列**。

```
SET / --set / FOR / EACH / glob / pages / sheets
        │  展开成 N 个物理读取
        ▼
   ┌─────────┐   ┌─────────┐   ┌─────────┐
   │ east p1 │   │ east p2 │   │ west … │
   └────┬────┘   └────┬────┘   └────┬────┘
        └─────────────┼─────────────┘
                      ▼
              表 orders
              列：业务列 + _source + _region + _page + _sheet
                      │
                      ▼
              一份 SELECT / GROUP BY _region
```

查询层看不到展开。换 DuckDB 时，搬走的是这张普通表。

---

## 2. 三层脚本（查询仍是 SQL）

```
Bind   SET x = ...    --set x=...     环境变量
Source LOAD … FOR/EACH / 连接器展开    封闭小语言
Query  SELECT / JOIN / GROUP BY       引擎标准 SQL
```

不在 SELECT 里做宏替换，不发明 `APPLY SELECT` 方言。需要「同一分析、多个输出文件」时，用 `_source` 过滤或拆文件；脚本级 APPLY 留到以后，避免第三套语言。

---

## 3. 绑定：相似源只改参数

```sql
SET base = 'https://api.example.com';
SET region = 'east';

LOAD orders FROM '${base}/${region}/orders' WITH (
  format='json',
  json_path='data'
);
SELECT status, COUNT(*) FROM orders GROUP BY status;
```

```bash
sqlxls report.sql --set region=west --set base=https://api.example.com
```

`${name}` 查找顺序：**SET / FOR / --set → 环境变量**。定位符里未定义会报错；`headers` 里未定义则留空（兼容 `${TOKEN}`）。

查询文本 **不** 做替换。要把维度带进分析，用展开生成的 `_region` / `_source` 列。

---

## 4. 列表展开：FOR / EACH

### 4.1 `FOR`：模板 × 取值集合

```sql
LOAD orders FROM 'https://api.example.com/${region}/orders'
WITH (format='json', json_path='data')
FOR region IN ('east', 'west', 'north');

SELECT _region, SUM(amount) AS total
FROM orders
GROUP BY _region;
```

取值集合：

| 写法 | 含义 |
|------|------|
| `IN ('a', 'b')` | 字面量列表，元素可再含 `${}` |
| `IN 1..12` | 闭区间整数 |
| `IN 1..10 STEP 2` | 步长 |
| `IN GLOB './sales_*.csv'` | 匹配到的路径（排序后） |

多个 `FOR` **嵌套**（后者可以用前者的变量），不是预先笛卡尔再求值：

```sql
LOAD t FROM './${region}/${month}.csv'
FOR region IN ('east', 'west')
FOR month IN GLOB './${region}/*.csv';  -- 不推荐混用，见下
```

更清晰的是：目录用 glob 连接器，区域用 FOR，分页用 `page_param`。

来源列：`FOR region` → `_region`；最终定位符 → `_source`（若尚未有同名列）。

### 4.2 `EACH`：一组定位符

```sql
LOAD sales FROM EACH (
  'jan.csv',
  'feb.csv',
  'https://api.example.com/mar'
) WITH (format='csv');   -- URL 仍走 HTTP 传输
```

```sql
LOAD sales FROM EACH GLOB './sales_*.xlsx' WITH (format='excel', sheet='Sheet1');
```

`EACH` 等价于「隐式 FOR 定位符列表」，结果同样 UNION 成一张 `sales`，带 `_source`。

不要为每个文件 `LOAD jan` / `LOAD feb`。一张表 + `GROUP BY _source` 即可。

---

## 5. 连接器内展开（知道何时停止）

脚本级 FOR 适合 **有限已知集合**。分页、通配、全部 Sheet 更适合连接器：它们能在空页 / 无文件时停下。

| 机制 | 写法 | 来源列 | 停止条件 |
|------|------|--------|----------|
| 目录/通配 | `'./sales_*.csv'` 或 `format='glob'` | `_source` | 无匹配则报错 |
| 全 Sheet | `sheet='*'` 或 `sheet='all'` | `_sheet` | 工作簿内全部表 |
| 页码 | `page_param='page', page_from=1, page_to=50` | `_page` | 空页或到达 `page_to` |
| 偏移 | `offset_param='offset', offset_step=100` | `_offset` | 空页 |

HTTP 例：

```sql
LOAD orders FROM 'https://api.example.com/orders'
WITH (
  format='json',
  json_path='data',
  page_param='page',
  page_from=1,
  page_to=100,
  page_size_param='limit',
  page_size=200,
  stop='empty'          -- 默认；stop='never' 则跑满 page_to
);
```

`http(s)` 定位符 **始终走 HTTP 连接器**；`format='json'` 只表示按 JSON 解码，分页选项仍然合法。

关闭来源列：`include_source=false`。

---

## 6. 高维组合怎么选

同时变化多个轴时，用 **嵌套** 而不是把所有轴都写成 FOR 笛卡尔：

```
外层：区域 / 租户 / 日期     →  FOR 或 EACH
内层：该实体下的分页         →  page_param（每个 URL 自己停）
文件集：同目录同构           →  glob，不必 FOR
工作簿：同构多表             →  sheet='*'
```

推荐：

```sql
SET base = 'https://api.example.com';

LOAD orders FROM '${base}/${region}/orders'
WITH (
  format='json',
  json_path='data',
  page_param='page',
  page_to=50
)
FOR region IN ('east', 'west');

SELECT _region, status, COUNT(*) AS n
FROM orders
GROUP BY _region, status;
```

每个 region 独立分页直到空页，再 UNION。分析仍然一条 SQL。

反模式：

- `FOR page IN 1..100` 写死页数，空页还继续打（除非你确实要固定页）。
- 每个文件 `LOAD t1` … `LOAD t12` 再 `UNION ALL` 手写。
- 在 SELECT 列表里拼 `read()`。

---

## 7. 明确不做（避免方言膨胀）

| 想法 | 为什么不做 |
|------|------------|
| Jinja / 整文件模板替换查询 SQL | 注入、引号、语言漂 |
| `APPLY SELECT … OVER EACH` | 第三套语言；UNION + `_source` 已覆盖主路径 |
| 为每个展开自动建 `orders_east` 表 | 正是用户要摆脱的命名爆炸 |
| 在查询层加自定义函数做循环 | 绑死引擎 |
| 静默忽略未知 WITH 选项 | 以后无法再占用这个名字 |

后续可加、但不进查询方言：`sqlxls.toml` catalog（命名源模板）、cursor 分页、`APPLY` 多文件导出。

---

## 8. 落地状态

| 能力 | 状态 |
|------|------|
| `SET` / `--set` / `${var}` | 已实现 |
| `FOR … IN` 列表 / 范围 / GLOB | 已实现 |
| `LOAD … FROM EACH` / `EACH GLOB` | 已实现 |
| 展开结果 UNION BY NAME + 来源列 | 已实现 |
| glob `_source`；JSON 多文件按列名合并 | 已实现 |
| `sheet='*'` + `_sheet` | 已实现 |
| HTTP `page_param` / `offset_param` 直到空页 | 已实现 |
| `sqlxls.toml` catalog | 未做 |
| cursor / Link header 分页 | 未做 |
| 每源一个输出文件的 APPLY | 未做 |
