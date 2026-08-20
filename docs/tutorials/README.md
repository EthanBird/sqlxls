# sqlxls 教程

按场景学，不必一次读完。先看 [使用手册](../USAGE.md) 查语法，再按下面挑一篇动手。

设计文档（给实现/演进看的，不是入门）：[SYNTAX.md](../SYNTAX.md)、[DYNAMIC.md](../DYNAMIC.md)、[ARCHITECTURE.md](../ARCHITECTURE.md)。

## 建议顺序

| # | 文档 | 你要解决的问题 |
|---|------|----------------|
| 1 | [安装与命令行](01-installation.md) | 下载、编译、`sqlxls --help`、`--strict`、导出 |
| 2 | [第一份脚本](02-first-query.md) | `LOAD` + `SELECT` 两层语言；探 schema |
| 3 | [Excel](03-excel.md) | sheet、skip、无表头 `header=false`、自定义 `columns`、全表、远程二进制 xlsx |
| 4 | [CSV 与编码](04-csv-encoding.md) | 分隔符、GBK/UTF-8、TSV、无表头 |
| 5 | [JSON](05-json.md) | `json_path`、字段并集、包装对象 |
| 6 | [HTTP API](06-http-api.md) | GET/POST、鉴权、分页、远程 CSV/Excel、HTTPS 自签 `insecure=true` |
| 7 | [相似源扇出](07-dynamic-sources.md) | `SET` / `FOR` / `EACH` / glob / 多 sheet |
| 8 | [日期窗口与转换](08-dates.md) | `DATE '…'..'…'`、`parse_date`、时间戳 |
| 9 | [清洗、JOIN、窗口](09-sql-cleaning.md) | 去重、类型、质量报告、多表关联 |
| 10 | [脚本与导出](10-export-and-scripts.md) | 多语句、`-o`、`--set`、`--explain` |
| 11 | [剪贴板、目录、假数据](11-clipboard-glob-mock.md) | clip、glob、`mock_data` |
| 12 | [常见问题](12-faq.md) | 报错对照、能力边界 |

每篇都可以单独复制示例去跑。交付脚本一律加 `--strict`。
