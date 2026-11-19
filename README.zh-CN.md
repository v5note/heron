# 苍鹭 Heron

> 中文文档 · [English](README.md)

**把散落在各处的 Agent Skill 收拢成一份受版本管理的副本，再按需分发，并让升级变成一条命令。**

Heron 面向同时使用 Claude Code、Codex、Cursor 等工具的开发者：Skill 分散在 `~/.claude/skills`、
`~/.codex/skills`、`~/.agents/skills` 和 N 个项目目录里，改一处其余仍是旧的，想升级又怕覆盖自己的改动。
Heron 把这些问题收成三件事：**收拢（adopt）→ 分发（project）→ 升级（upgrade）**。

- 支持 macOS 12+ 与 Windows 10 1803+（Linux 可运行，未作为首发目标）
- 图形界面 + 命令行，两者共用同一套逻辑，行为一致
- 所有改动可预览、可回滚；删除任何东西前都会先备份

---

## 它解决什么

| 痛点 | Heron 的做法 |
|---|---|
| Skill 散落在四五个隐藏目录 | `heron scan` 一次扫全，列出"已纳管 / 未纳管" |
| 同一个 Skill 有多份拷贝 | `heron adopt` 收拢为**一份**实体，原位置变成链接 |
| 不知道客户端到底有没有加载 | `heron verify` 区分「生效中 / 未被扫描 / 被压制 / 坏链」 |
| 上游更新了不知道 | `heron outdated` 对比上游提交 |
| 升级会覆盖我改过的行 | 三方合并，本地改动与上游改动都保留 |
| 团队每人 Skill 各不相同 | `heron.lock` 导出/复现 |
| 想从 GitHub / 市场装 Skill | `heron install` + 五个内置市场 |

---

## 快速开始

### 1. 环境要求

- **git 2.38+**（三方合并需要 `git merge-tree --write-tree`）。没有安装时 Heron 会给出安装指引，而不是静默降级。
- macOS 12+ / Windows 10 1803+（Windows 需要 WebView2，Win11 自带）。

### 2. 安装

**桌面应用**（推荐）：从发布页下载 `Heron.dmg`（macOS）或 `Heron_x64-setup.exe`（Windows）。

**命令行**：与桌面应用一起分发，也可单独使用：

```bash
heron --help
```

### 3. 三步走

```bash
heron scan        # 扫一遍本机，看看有什么
heron detected    # 列出 Heron 还没接管的 Skill
heron adopt --all # 全部收拢（会先备份、再去重）
```

收拢之后，原来的目录**不会消失**，而是变成指向 Heron 存储区的链接：

```text
收拢前                          收拢后
~/.agents/skills/pdf/     →     ~/.agents/skills/pdf → <数据目录>/store/pdf
~/.claude/skills/pdf/     →     ~/.claude/skills/pdf → <数据目录>/store/pdf   （重复的自动去重）
```

> 第一次使用务必先跑 `heron adopt --all --dry-run`，看清每一步再执行。

---

## 命令总览

| 命令 | 作用 |
|---|---|
| `heron scan` | 扫描所有已知位置，刷新索引 |
| `heron detected` | 列出未纳管的 Skill |
| `heron adopt <slug...> \| --all` | 收拢到存储区 |
| `heron list` | 已纳管的 Skill |
| `heron project <slug> [--on <target>]` | 投影到某个落点 |
| `heron unproject <slug> [--on <target>]` | 移除投影（保留存储副本） |
| `heron verify` | 该 Skill 在客户端里到底会不会生效 |
| `heron doctor` | 只读体检：重复、坏链、病态嵌套、来源丢失 |
| `heron install <url>` | 从 Git 仓库安装 |
| `heron outdated` / `upgrade` / `rollback` | 检查更新 / 三方合并升级 / 回滚 |
| `heron diff` / `conflict` / `resolve` | 查看改动 / 查看冲突 / 解决冲突 |
| `heron market list\|search\|plan` | 五个内置市场 |
| `heron lock export\|diff\|apply` | 团队协作 |
| `heron projects` | 发现含客户端配置的项目 |

所有命令都支持 `--json`（给脚本用）与 `--data-dir`（隔离数据目录）。

完整用法见 **[上手指南](docs/GUIDE.md)** 与 **[命令参考](docs/REFERENCE.md)**。

---

## 设计取舍（值得先知道）

**Heron 从不擅自破坏。** 以下行为是刻意设计，而不是遗漏：

- 收拢前一定先备份到 `<数据目录>/backups/`；备份失败则整个操作中止。
- 投影遇到**别人的实体目录**时停下并报 `E_NOT_MANAGED`，不会覆盖。
- copy 模式下如果目标文件被外部改过，报 `E_DRIFTED` 而不是覆盖你的修改。
- 病态嵌套（`skills/.claude/skills/…`）**只报告，不自动整理**。
- 从上游安装的 Skill，首次安装来源不是可信来源时只展示内容摘要，不执行任何脚本。

**Heron 不会假装能做的事。**

- 市场分两档：`可搜索`（skills.sh、ClawHub 能直接安装）与 `仅浏览器`（qoder、SkillHub、SkillsMP 只能打开网页，然后粘贴 Git 地址回来）。
- 未接管来源的 Skill 在 `outdated` 中显示 `not tracked`、版本为 `0.0.0`，不会被当成"可以升级"。
- 客户端原生扫描（往客户端配置里加扫描目录）默认关闭；未经验证的客户端永远不会被写入。

---

## 数据放在哪

| 平台 | 数据目录 |
|---|---|
| macOS | `~/Library/Application Support/io.greyheron.heron` |
| Windows | `%LOCALAPPDATA%\heron` |
| Linux | `~/.local/share/heron` |

其中 `store/` 是唯一真相源（每个 Skill 一个 Git 仓库），`backups/` 是安全网，`trash/` 是软删除区。
整个索引（`heron.db`）可以随时从存储区重建。

> `~/.agents/skills` **不是** Heron 的存储区。Heron 有自己的默认落点 `~/.heron/skills`，
> `~/.agents/skills` 只作为可选的"共享映射点"，因为那个目录同时被 `npx skills` 使用。

---

## 从源码构建

```bash
# 依赖：Rust stable、Node 22、pnpm 10
pnpm --dir ui install
pnpm --dir ui build
cargo build -p heron-cli --release   # 命令行 → target/release/heron
cargo build -p heron-app --release   # 桌面应用
```

本地校验（等价于 CI）：

```bash
scripts/dev.sh check   # fmt + clippy
scripts/dev.sh test    # 全部测试
scripts/dev.sh ui      # 前端类型检查 + i18n 一致性 + 构建
```

## 项目结构

```text
crates/
  heron-core        领域模型（无 IO）
  heron-platform    唯一允许平台分支的层：链接、锁、路径、目录
  heron-store       SQLite 索引与迁移
  heron-scan        遍历、frontmatter 解析、内容指纹
  heron-vcs         每个 Skill 一个 Git 仓库、三方合并
  heron-git         远端裸仓缓存与子目录检出
  heron-source      URL 解析、catalog、市场适配器
  heron-projection  投影计划 / 执行 / 可见性判定
  heron-service     用例编排（CLI 与 GUI 共用）
  heron-cli         命令行
src-tauri/          Tauri 2 外壳（薄适配层）
ui/                 Vue 3 前端
docs/               设计、实现、指南与决策记录（ADR）
```

## 文档

- [设计文档](docs/DESIGN-heron-v2.md) —— 为什么这样设计
- [实现文档](docs/IMPLEMENTATION-heron-v1.md) —— 怎么落地、按什么顺序
- [上手指南](docs/GUIDE.md) —— 从一个乱糟糟的机器开始
- [命令参考](docs/REFERENCE.md) —— 每条命令、每个错误码
- [决策记录](docs/adr/) —— 实现中踩到的坑与取舍

## 许可

MIT OR Apache-2.0
