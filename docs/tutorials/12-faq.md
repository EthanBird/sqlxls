# 12 · 常见问题

## 报错对照

| 信息大意 | 处理 |
|----------|------|
| 字符串范围语义不明 / 需要 `DATE` | 写成 `FOR d IN DATE '2024-01-01'..'2024-01-31'` |
| 未定义的变量 `${x}` | `SET x=...` 或 `--set x=...` 或环境变量 |
| 文本不是合法 UTF-8 | `encoding='gbk'`，或让 HTTP 带 `charset=gbk` |
| 未知 encoding | 常用 `utf-8` `gbk` `gb18030` `gb2312` `big5` |
| format 不支持选项 `foo` | 该 format 的关闭集合里没有这个名字；不要指望被忽略 |
| API 返回了 HTML | URL/鉴权不对，登录页被当成数据了 |
| 无法识别的 API 响应 | 显式 `format='json'\|'csv'\|'excel'`；Excel 看魔数，CSV 看编码 |
| 表函数不能写在 SELECT/WHERE | 先 `LOAD`，查询只引用表名 |
| `read_text` 不能当表 | 它是标量，嵌在 `body=read_text('a.json')` |
| 展开后没有任何数据源 | `FOR`/`EACH`/glob 结果为空，或日期区间起始晚于结束 |
| `no such column` | 中文列加 `"双引号"`；先 `PRAGMA table_info`。没有表头时默认仍会把第一行当列名，应写 `header=false`；自定义名用 `columns='…'` |
| LOAD 表名非法 | 表名只能 `[A-Za-z_][A-Za-z0-9_]*` |

`--explain` 看展开了几个 URL/文件。

## 为什么查询里不能 ${}？

避免把 SQL 变成模板语言（引号、注入、和引擎绑死）。维度用 `_region` / `_source` / `_d`。绑定只作用于 Source 定位符和部分选项字符串。

## 为什么日期必须写 DATE？

`'a'..'b'` 看起来像字符串切片。强制 `DATE` 后，整数 `1..12` 仍是月份序号或批号，不会被猜成日历。

## UTF-8 和 GBK 会自动猜吗？

UTF-8 是默认。HTTP `charset=` 会用。**不会**在 UTF-8 失败时偷偷当 GBK（以免把随机二进制解成乱汉字）。失败就按提示加 `encoding=`。

## 远程 Excel 一定要 .xlsx 后缀吗？

不必。看文件头：xlsx=`PK`，xls=OLE。`octet-stream` 也可以。HTML 不行。

## Excel / CSV 没有表头怎么办？

默认第一行是列名。整张都是数据时写 `header=false`，列名是 `col_0`, `col_1`, …。要自己起名：

```sql
LOAD t FROM 'raw.xlsx' WITH (format='excel', header=false, columns='id,name,金额');
```

只写 `columns=` 仍会把第一行当表头丢掉。文件里已有表头、只想改名：`columns='a,b'` 即可。CSV 同样。

## 一次能跑多大？

全表进内存 SQLite。笔记本可交互的几十万～百万行通常可以；很大的 xlsx 先拆 CSV 或加过滤后再导入。没有谓词下推进 xlsx 解析。

## 能不能写回原来的 Excel？

不能。用 `-o 新文件.xlsx`。公式、格式、宏不会保留。

## 能不能直连数据库？

目前没有 `read_sql`。从库里导出 CSV/JSON 再 `LOAD`，或等后续版本。

## HTTP 分页停不住？

确认空页真的是 `[]` 或空表。包装在 `{data:[]}` 时要设对 `json_path`，否则「空业务数组」可能仍被当成一行对象。`stop='empty'` 看的是解码后的**行数**。

## 和 pandas 怎么选？

表格文件 / 接口 → SQL 清洗导出：用 sqlxls。复杂建模、绘图、机器学习：pandas 或其它。不要为「读个 xlsx 做个 JOIN」先写解析器。

## 相关文档

- [使用手册](../USAGE.md)
- [教程目录](README.md)
- [动态源设计](../DYNAMIC.md)
- [语法规范](../SYNTAX.md)
