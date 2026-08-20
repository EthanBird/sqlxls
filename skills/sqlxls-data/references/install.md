# 安装与运行环境

## 获取 sqlxls

优先顺序（`scripts/ensure-sqlxls.sh` 已实现）：

1. 环境变量 `SQLXLS` 指向可执行文件
2. `PATH` 里的 `sqlxls`
3. 当前 git 仓库的 `target/release/sqlxls`（从本技能包或 cwd 向上找）
4. 从 GitHub Releases 最新版下载到 `${XDG_CACHE_HOME:-$HOME/.cache}/sqlxls/bin/sqlxls`

手动下载：https://github.com/EthanBird/sqlxls/releases

| 平台 | 文件 |
|------|------|
| Linux x86_64 | `sqlxls-x86_64-unknown-linux-gnu` |
| macOS Apple Silicon | `sqlxls-aarch64-apple-darwin` |
| macOS Intel | `sqlxls-x86_64-apple-darwin` |
| Windows x86_64 | `sqlxls-x86_64-pc-windows-msvc.exe` |

放进 `PATH` 后执行 `sqlxls --help`。源码编译需要 Rust 1.88+：`cargo build --release`。

## 把技能包装到其它项目

本目录就是技能包。复制到 Agent 能发现的位置：

```bash
# 当前仓库（已通过 .agents/skills/sqlxls-data 链接，一般不用再拷）
# 其它项目
cp -R skills/sqlxls-data <other-repo>/.agents/skills/sqlxls-data

# 用户全局
cp -R skills/sqlxls-data ~/.agents/skills/sqlxls-data
# 或
cp -R skills/sqlxls-data ~/.cursor/skills/sqlxls-data
```

Cursor / Claude / Codex 会从 `.agents/skills/`、`.cursor/skills/`、`~/.agents/skills/`、`~/.cursor/skills/` 加载。文件夹名必须是 `sqlxls-data`，且内含 `SKILL.md`。

## HTTP 鉴权

```bash
export SQLXLS_BEARER_TOKEN='...'          # 自动 Authorization: Bearer
export TOKEN='...'                        # 供 SQL 里 ${TOKEN} 展开
```

headers 必须是 JSON 对象字符串：

```sql
headers='{"Authorization":"Bearer ${TOKEN}","X-Request-Id":"agent-1"}'
```

不要把 token 写进要提交的 `.sql`。Agent 环境若禁止出网，HTTP 源会失败——改成本地下载的 JSON/CSV 再 `LOAD`。

## 能力边界（不要假装能做）

- 无 `read_sql`（不能直连 Postgres/MySQL）
- 无 S3 / Parquet / 结果缓存 / `sqlxls.toml` catalog
- 无 HTTP cursor / Link 分页（`page_param` / `offset_param` 可以）；无 REPL、无独立 `--schema` 旗标（用 `PRAGMA table_info`）
- 全量进内存；超大 xlsx 先让用户切分或导出 CSV
- 不写回原工作簿
