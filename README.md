# 📊 sqlxls

**sqlxls** 是一个基于 Rust 开发的、使用 SQL 语法来处理 Excel/CSV/剪贴板数据的命令行轻量级框架。

它通过在内存中利用 SQLite 构建临时表，让你能够使用标准且强大的 SQL 语句（如 `JOIN`、`GROUP BY`、子查询等）直接对本地文件或剪贴板数据进行查询、关联和分析，并支持输出为多种常见格式。

## ✨ 特性

- **多样化数据源读取**：
  - `readexcel`: 读取单个 Excel 文件。
  - `readdir`: 批量读取并合并某个目录下所有匹配通配符的 Excel 文件。
  - `readclipboard`: 极度方便的剪贴板读取功能，支持读取纯文本表格、CSV 内容，甚至可以直接解析在资源管理器中复制的 Excel 文件。
- **内置假数据引擎**：通过 `mock_data` 函数在 SQL 中快速生成测试用的姓名、电话、邮箱等假数据。
- **强大的 SQL 引擎**：底层基于 SQLite，支持所有的标准 SQL 语法。
- **多格式输出**：
  - 默认在终端中渲染美观的数据表格（探索数据时极佳）。
  - 支持通过参数直接将结果导出为 `.csv`、`.json` 或 `.xlsx`。

## 📦 安装与编译

确保你的电脑上安装了 Rust 环境，然后在项目根目录下运行：

```bash
cargo build --release
```
编译成功后，可执行文件将生成在 `target/release/sqlxls`。

## 🚀 简单的使用说明

`sqlxls` 的核心理念是将数据加载函数直接嵌入到 SQL 的 `FROM` 子句中。

**基本语法：**
```bash
sqlxls "你的 SQL 语句" [-o 输出文件路径]
```

### 1. 读取并查询 Excel
使用 `readexcel('文件路径', 'Sheet名称')` 作为表名：
```bash
sqlxls "SELECT * FROM readexcel('data.xlsx', 'Sheet1') WHERE id > 100"
```
*(可选：你可以传入第三个参数 `'str'`，将所有单元格强制以字符串形式读取)*：
```bash
sqlxls "SELECT * FROM readexcel('data.xlsx', 'Sheet1', 'str')"
```

### 2. 批量合并目录下的 Excel
使用 `readdir('通配符路径', 'Sheet名称')` 可以把结构相同的多个表格自动合并为一张大表进行查询：
```bash
sqlxls "SELECT category, SUM(price) FROM readdir('./sales_*.xlsx', 'Sheet1') GROUP BY category"
```

### 3. 直接查询剪贴板数据 (极速体验)
复制了一段带有制表符或逗号分隔的文本（或是直接复制了某个 `.xlsx` / `.csv` 文件），然后运行：
```bash
sqlxls "SELECT * FROM readclipboard() LIMIT 10"
```

### 4. 复杂的多表 JOIN
你可以在一条 SQL 中调用多个读取函数，实现跨文件的数据关联：
```bash
sqlxls "
SELECT a.id, a.name, b.department 
FROM readexcel('users.xlsx', 'Sheet1') AS a
JOIN readclipboard() AS b ON a.id = b.user_id
"
```

### 5. 生成模拟数据 (Mock Data)
内置了假数据生成器，非常适合用来测试。支持的类型有：`name` (姓名), `phone` (电话), `email` (邮箱), `company` (公司), `city` (城市)。
格式为：`mock_data(生成行数, '列名:类型', ...)`
```bash
sqlxls "SELECT * FROM mock_data(10, '用户名:name', '联系方式:phone', '所在城市:city')"
```

### 6. 导出结果
如果不带任何参数，结果会在终端以 ASCII 表格打印出来。你可以使用 `-o` 或 `--output` 将查询结果导出：
```bash
# 导出为 Excel
sqlxls "SELECT * FROM readexcel('data.xlsx', 'Sheet1')" -o result.xlsx

# 导出为 CSV
sqlxls "SELECT * FROM readexcel('data.xlsx', 'Sheet1')" -o result.csv

# 导出为 JSON (常用于给 API 准备数据)
sqlxls "SELECT * FROM readexcel('data.xlsx', 'Sheet1')" -o result.json
```

## 📝 TODO List

- [ ] **新增外部数据源支持**：计划增加 `readmysql()` 和 `readapi()` 等扩展功能。
- [ ] **剪贴板格式增强**：支持通过参数自定义剪贴板纯文本的解析分隔符（目前自动猜测制表符或逗号）。
- [ ] **内存优化**：在处理超大型 Excel 时优化 `calamine` 与 `rusqlite` 的内存占用，提供流式读取的可能。
- [ ] **写回/更新功能**：探索不仅仅是 `SELECT`，而是使用 `UPDATE`/`INSERT` 将处理完的数据直接写回 Excel 的实现方案。
- [ ] **CI/CD**：配置 GitHub Actions，提供 Windows / macOS / Linux 跨平台的预编译可执行文件下载。