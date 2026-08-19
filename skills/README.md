# sqlxls Agent Skill

给 AI Agent 用的数据处理技能包（[Agent Skills](https://agentskills.io) / Cursor Skills）。

Agent 读到 `SKILL.md` 后，应使用 **sqlxls CLI** 对 Excel / CSV / JSON / HTTP 等表格做 SQL 清洗与分析，而不是默认改用 pandas。

## 目录

```
sqlxls-data/
├── SKILL.md                 # 工作流与硬性规则（激活时加载）
├── scripts/                 # ensure / probe / run
├── references/              # 语法、SQLite 方言、配方、安装
└── assets/templates/        # 可改路径直接跑的 SQL
```

## 安装到其它项目或本机

```bash
# 项目级（Cursor / Claude / Codex 会自动发现）
cp -R skills/sqlxls-data <repo>/.agents/skills/sqlxls-data

# 用户全局
cp -R skills/sqlxls-data ~/.agents/skills/sqlxls-data
```

本仓库已通过 `.agents/skills/sqlxls-data` 符号链接指向这里，克隆后即可被 Agent 发现。

手动调用：在 Agent 对话里输入 `/sqlxls-data`。
