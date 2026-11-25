> ⚠️ **已归档**：本文档已被新版取代，请阅读 [设计文档 v2](../DESIGN-heron-v2.md)。保留此文件仅作历史参考。

# 苍鹭 (grey-heron / heron) — 本地 Skill 管理器 · 设计文档（v0.2 归档）

版本: v0.2 草案（按客户端扫描路径实测结果修订，见 §1.1 / §3 / §13）  
日期: 2026-11-19  
状态: 待评审 —— 评审通过后再写《实现文档》

---

## 0. 一句话定位

一个跨平台的命令行工具，把散落在本机各处、以及来自 Git 远程仓库的 Agent Skill **收拢成一份唯一真相源**，再按目标（Claude Code / Codex / .agents / 各项目）分发出去，并且让"升级"变成一条命令。

名字取自苍鹭：站在一处，看着整片水面。

---

## 1. 问题陈述（基于对本机的实测）

以下不是假设，是在这台 macOS 机器上扫出来的真实情况：

| 现象 | 实测证据 |
|---|---|
| Skill 散落多处 | `~/.claude/skills` (2)、`~/.codex/skills` (2)、`~/.agents/skills` (45)；另有 ≥8 个项目各自带 `.claude`/`.codex`/`.agents` |
| 总量已经失控 | 全盘扫到 **537 个 `SKILL.md`**（去重前，含 worktree / vendor 副本） |
| 同一个 skill 多份拷贝 | `find-skills` 在 `.claude/skills` 与 `.agents/skills` 都有；`huashu-*` 系列至少 3 处 |
| 已经有人在手工用 symlink 补救 | `~/.claude/skills/find-skills ⇒ ../../.agents/skills/find-skills`；`~/.agents/skills/huashu-motion ⇒ ~/.agents/huashu-motion/skills/huashu-motion` |
| 出现病态嵌套 | `~/.agents/skills/.agents/skills/{huashu-art-motion,huashu-design}` —— 项目结构被整体拷进了 skill 目录 |
| 第三方缓存里也有 skill | `~/.codex/vendor_imports/skills/skills`、`~/.codex/.tmp/bundled-marketplaces/...`、VS Code 扩展目录里的 `.claude` |
| 元数据不统一 | 有的 frontmatter 有 `metadata.version`（如 reverse-engineer-anything: `"42"`），绝大多数没有版本号 → **无法判断新旧，也就无从升级** |
| 已有 marketplace 机制但互不相通 | Claude 的 `~/.claude/plugins/known_marketplaces.json`（GitHub repo 为源）自成一套，Codex 另有 bundled marketplace，两边不能共用一个仓库地址 |
| **各客户端扫描路径不一致**（决定性发现） | 实测：Claude Code 只扫 `~/.claude/skills/`、项目 `.claude/skills/` 和启用的插件，**根本不扫 `~/.agents/skills/`** —— 那边 45 个 skill 它只看到 2 个，而这 2 个恰好都在 `.claude/skills` 有入口。Codex 则原生识别 `~/.agents` |
| 同名冲突由优先级静默裁决 | `code-review` 在 `~/.agents/skills` 与插件 `code-review:code-review` 同时存在时，**插件优先，agent 目录那份被无声忽略** |

### 1.1 痛点归纳

* **P1 找不到** —— "我到底有哪些 skill？哪个在用？" 无全局视图。
* **P2 改不动** —— 同一 skill 有 N 份拷贝，改一处其余仍是旧的。
* **P3 升不了** —— 没有版本记录、没有来源绑定，无法知道上游有没有更新，更无法安全覆盖。
* **P4 装不全** —— 想把一个 skill 同时给 Claude 和 Codex 用，只能手工 cp 或手工 ln -s。
* **P5 分不清该不该删** —— worktree、vendor、编辑器扩展里的 skill 是垃圾还是资产？
* **P6 团队协作难** —— 项目里的 `.claude/skills` 每人各改各的，很快漂移。
* **P7 装了却没生效** —— 客户端扫描路径不同 + 插件优先级压制，导致"文件明明在，模型看不见"，且失败无声。

> **关于 P4/P7 的实测修正（原 §13 第一风险已排除）**：链接机制本身完全可用 —— 经软链加载 `find-skills` 验证通过，Codex 对软链同样正常。所以真正的问题不是"能不能链"，而是"链到哪个客户端会扫的路径上"。这把技术风险从**架构级**降级为**配置表级**，代价是要维护一张精确的 per-client 兼容性矩阵（§12.1）。
>
> 附带两条硬约束：① 新建软链后需**重启会话**才会被重新扫描；② 全量挂 45 个意味着每次会话都载入 45 条 description，token 开销显著 —— 因此 `heron use` 默认按需在具体 target 上启用，并提供 `heron usage` 查看实际触发情况（见 §9）。

---

## 2. 目标与非目标

### 2.1 目标（v1）

* G1 **发现**：自动扫描并识别本机所有 skill 及其宿主类型，形成索引。
* G2 **收拢**：把选定的 skill 迁入统一 store，原位留链接，旧路径零破坏。
* G3 **分发**：一次导入，按需部署到任意 target（全局或某个项目）。
* G4 **来源**：支持默认远程仓库 + 用户自加仓库地址 + 直接给某个 GitHub/GitLab skill 地址。
* G5 **升级**：`heron outdated` 列出可升级项，`heron upgrade` 按 pin 策略更新，可回滚。
* G6 **体检**：校验 frontmatter、检测重复/冲突/病态嵌套/孤儿拷贝。
* G7 **跨平台**：macOS 与 Windows 一等公民，Linux 顺带可用（架构上不做任何平台特化假设）。

### 2.2 非目标（明确不做）

* 不做 GUI / TUI 优先（先 CLI；TUI 是 v2 的事）。
* 不接管 MCP server、plugin、subagent、hook 的管理（只管 skill；但要能"识别"它们以免误删）。
* 不做云端同步账号体系（Git 即同步；未来若要再加）。
* 不改造 Claude/Codex 自身的加载机制 —— **我们只产出它们本来就认识的文件布局**。
* 不在 v1 做签名验真 / SBOM。
* 不做 VS Code / JetBrains 插件。

---

## 3. 核心概念模型

这是整个设计的支点：**把"skill 是什么"和"skill 在哪里被使用"彻底分开。**

```
        Source (远程仓库, Git)
              │  add / install / upgrade
              ▼
      ┌───────────────┐
      │     Store     │   唯一真相源 ~/…/heron/store/<slug>/<version>/
      │  不可变版本    │   每个 slug 可有多个已下载版本
      └───────┬───────┘
              │  link (symlink / junction)
              ▼
      ┌───────────────┐
      │   Target      │   部署点 = <dir>/skills/<name>
      │ .claude       │   可以是全局 (~/.claude/skills)
      │ .codex        │   也可以是项目 (repoA/.claude/skills)
      │ .agents       │
      │ 项目级 …      │
      └───────────────┘
```

三个名词的精确定义：

* **Skill** —— 一个含 `SKILL.md` 的目录。身份由 `(slug, content_hash)` 决定，`slug` 默认取 frontmatter `name`，缺失则取目录名。
* **Store** —— 用户数据目录下的集中存放区。**只有 store 里的内容才是"安装"**；target 里出现的都是指向 store 的链接。这样"改一处 = 处处生效"，直接解掉 P2/P4。
* **Target** —— 任何 `<某目录>/skills/` 形态的落地点，带 `kind`（claude / codex / agents / generic）和 `scope`（global / project:<path>）。

外加三个约束：

* **Managed vs Unmanaged** —— 每一个进入 target 的条目都记录在 heron 的索引里。没记录的（比如 Claude 自己写的、vendor 缓存里的）一律标 `unmanaged`，**永远不主动改动**，只报告。这条规则保证工具可信、可撤销。
* **Immutable version** —— store 里 `<slug>/<version>` 一旦落地就不再就地修改；升级是新下载一个版本目录 + 重新指向链接。这让回滚变成改一个指针，而不是恢复文件。
* **★ Installed ≠ Visible** —— 这是本项目最容易被忽略的一条。skill 落到某个 target 只是"安装完成"，不代表客户端会加载它。可见性取决于两张表：
  1. **扫描路径表** —— Claude Code 不扫 `.agents`，Codex 扫；所以同一个 skill 给两边用必须在 `.claude/skills` 也有入口。
  2. **优先级压制表** —— 同名时插件 > 项目 `.claude/skills` > 全局 `.claude/skills`，被压制的那份是"装了但永久无效"。

  因此每次 `use` 之后 heron 必须做一次**生效校验**（§9 `heron verify`），把"静默无效"变成显式报错。这一条直接针对 P7，也是 §13 风险清单里唯一还活着的架构级风险。

### 3.1 Per-client 兼容性矩阵（设计基线，实现时需真机复核）

| 客户端 | 原生扫描路径 | 认 symlink/junction | 项目级 | 同名优先级 | 生效时机 |
|---|---|---|---|---|---|
| Claude Code | `~/.claude/skills`、`<proj>/.claude/skills`、启用的 plugin | ✅ 实测通过 | ✅ | plugin > 项目 > 全局 | **需重启会话** |
| Codex | `~/.codex/skills`、`~/.agents/skills`、`<proj>/.codex` | ✅ 用户实测 | ✅ | 待测 | 待测 |
| 通用 agent (`npx skills`) | `~/.agents/skills` | ✅（既有惯例） | ✅ | 无 plugin 概念 | 待测 |
| Cursor / opencode 等 | 各自路径，多数含 `.cursor`、`bundled/skills` | 待测 | 待测 | 待测 | 待测 |

矩阵以数据形式存在（`clients.toml`，随二进制打包 + 用户可覆盖），而不是硬编码在分发逻辑里 —— 客户端版本一改扫描路径就变，**这张表必须是可热更的配置**。

---

## 4. 决策：技术选型

本机现状：node v22 ✅ / npm ✅ / bun 1.3 ✅ / cargo+rustc 1.99 ✅ / go ⚠️(异常) / python 3.9 ⚠️(偏旧) / java 21 ✅ / git 2.50 ✅

| 方案 | 单文件分发 | Windows 无依赖运行 | 冷启动 | Git 操作 | 生态契合 | 结论 |
|---|---|---|---|---|---|---|
| **Rust 原生二进制** | ✅ | ✅ | 极快 | 需 libgit2/git 子进程 | 一般 | **推荐** |
| Node SEA / pkg | ❌ 体积大且脆 | ✅ | 慢 | simple-git | ✅ JS 生态 | 备选 |
| Go + cobra | ✅ | ✅ | 快 | go-git | 一般 | 可行（本机 go 当前异常） |
| Python + uv | ❌ 要解释器 | ❌ 最麻烦 | 慢 | GitPython | ❌ | 不推荐 |

**选型：Rust 单一静态二进制**，理由：

1. Mac/Windows/Linux 三平台交叉编译是 Rust 的日常，产物就是一个可执行文件，用户不需要装任何运行时 —— 这正好命中"前期只做 Mac 和 Windows，要用跨平台技术"。
2. 本工具的本质是**文件系统 + Git + 索引数据库**，性能敏感点在遍历 537+ 目录和并发 Git 拉取上，原生二进制明显占优。
3. SQLite 嵌入（`rusqlite` bundled）给出零配置索引库，避免 JSON 全量读写随规模退化。

关键 crate 意向（实现文档细化）：`clap` (CLI)、`camino`/`pathdiff` (路径)、`rusqlite` (索引)、`gix` 或子进程 `git` (远端)、`rayon` (并行扫描)、`notify` (watch)、`similar` (diff)。

> 若团队更熟 TypeScript，备选方案是 Bun + `bun build --compile` 出单文件 —— 架构分层完全一致，仅替换 §11。这一点请在评审时拍板。

---

## 5. 磁盘布局

```
~/Library/Application Support/heron/        # Windows: %APPDATA%\heron ; Linux: ~/.local/share/heron
├── heron.db                                # 索引：skill / target / link / source / event
├── settings.toml                           # 用户配置（默认 target、pin 策略、仓库列表）
├── sources.toml                            # 远程仓库注册表（可由 CLI 管理，独立于 settings）
├── store/
│   ├── registry.lock                       # 每个 source 上次同步到的 commit SHA
│   ├── index/                              # 缓存的远端 catalog JSON
│   └── <slug>/<version>/                   # 不可变的 skill 内容
├── cache/git/<hash-of-url>/                # bare clone 工作区（可安全删除重建）
└── log/heron.log

<Target 目录示例>
~/.claude/skills/find-skills  ->  …/heron/store/find-skills/1.4.0   (symlink / junction)
~/proj/.agents/skills/wechat-mp     ->  …/heron/store/wechat-mp/2.0.1
```

约定：链接失效（store 里版本被清理）时 heron 报 `broken-link` 而不静默删除，交给人决定。

---

## 6. 配置文件

### 6.1 `settings.toml`

```toml
[general]
auto_discover = true          # 启动时增量刷新索引
default_targets = ["claude", "codex", "agents"]
conflict_policy = "ask"       # ask | keep | overwrite | side-by-side

[upgrade]
pin = "semver"                # none | semver | exact （见 §8）
verify_hash = true            # 内容有未提交改动时拒绝覆盖
keep_versions = 3             # 每个 slug 保留的历史版本数

[scan]
exclude = [
  "**/node_modules/**", "**/.git/**", "**/worktrees/**",
  "~/.codex/vendor_imports/**", "~/.vscode/extensions/**",
]
max_depth = 8
```

### 6.2 `sources.toml` —— 三种来源类型统一表达

```toml
[[source]]
id = "heron-default"
type = "registry"                     # A. 注册表仓库（自带 catalog）
url = "https://github.com/heron-ai/skills.git"
enabled = true
trusted = true                        # 默认仓库：免交互确认

[[source]]
id = "my-team"
type = "registry"
url = "git@git.internal.team:ai/skills.git"
ref = "main"
prefix = "skills/"                    # 仓库内 skill 所在子目录

[[source]]
id = "adhoc-anthropic"
type = "repo"                         # B. 直接给一个代码仓库，里面扫 SKILL.md
url = "https://github.com/anthropics/skills.git"
subpath = "document-skills"

[[source]]
id = "one-off"
type = "skill"                        # C. 直接给单个 skill 的地址
url = "https://github.com/foo/bar/tree/v2.1.0/skills/logo-design"
```

三种类型对应你提的三个需求：默认仓库(A)、用户自己加仓库地址(A/B)、skill 的 GitHub 地址(C)。

---

## 7. 远程仓库规范（关键取舍）

这里必须做一个决定：**是套用 Anthropic 已有的 marketplace 格式，还是自定义。**

建议：**兼容读取，自有扩展。**

* 读取侧：能解析 `~/.claude/plugins/known_marketplaces.json` 风格的 GitHub repo 源，并把 Claude plugin 仓库里的 `skills/` 直接当注册表消费 → 用户已有的仓库地址立刻可用，不必迁移。
* 生产侧：定义 heron 自己的 `heron.registry.toml`（放在仓库根），因为现成格式缺我们升级流程必需的两样东西 —— **版本历史** 和 **内容哈希**。

```toml
# heron.registry.toml  （位于注册表仓库根）
schema = 1
generated_at = "2026-11-19T00:00:00Z"
generator = "heron-publish/0.1.0"

[[skill]]
slug = "wechat-mp"
name = "WeChat MP Toolkit"
version = "2.0.1"
path = "skills/wechat-mp"                       # 相对仓库根
sha256 = "9f2c…"                           # 目录规范化内容的哈希
tags = ["document", "official"]
min_schema = 1
changelog_url = "https://github.com/heron-ai/skills/releases/tag/wechat-mp-2.0.1"

[[skill]]
slug = "wechat-mp"
version = "2.0.0"
path = "skills/wechat-mp"
sha256 = "1a7b…"                            # 允许同一 slug 多版本共存 → 支持降级
```

版本语义取舍：

* **Git tag 型**（`wechat-mp-v2.0.1`）适合发布节奏明确的官方仓库。
* **commit-SHA 型**适合个人/团队仓库 —— 无需维护 tag，`pin=none` 时永远跟 HEAD。
* heron 两者都支持，`catalog` 里以 `(version, sha256, commit)` 三元组存在；**没有版本号的 skill 一律视为 `0.0.0+<shortsha>`**，并在 `doctor` 里提示补 `metadata.version`。这直接解决 §1 里"绝大多数 skill 没有版本号所以没法升级"的核心障碍。

---

## 8. 升级模型

`upgrade` 是最容易做错的部分，因此把策略显式建模成三档 `pin`：

| pin | 行为 | 适用 |
|---|---|---|
| `exact` | 锁死某版本，永不自动更新，`outdated` 也不报警 | 生产项目、已被改过的 skill |
| `semver` | 在允许的 major 范围内取最新 | 默认推荐 |
| `none` | 跟随 source 的 ref HEAD | 个人仓库、开发中的 skill |

流程：

```
heron outdated
  1. 增量 fetch 各 enabled source 的 catalog（bare clone，浅历史）
  2. 对比 db 中 installed_version vs catalog 最新 version
  3. 输出表格 + 分类：upgradable / pinned / local-modified / upstream-deleted

heron upgrade [slug...] [--all] [--dry-run]
  a. 本地改动检测：规范化后 hash(store 内容) != db.content_hash
       → 标 local-modified，默认跳过并提示 fork（见 §9 fork）
  b. 下载新版本到 store/<slug>/<newver>，校验 sha256
  c. 事务性切换：更新所有指向该 slug 的链接 + db 记录
  d. 任一步失败 → 回滚链接指针，旧版本仍在
  e. 超出 keep_versions 的旧版本进回收队列，--prune 才真删
```

失败面清单（实现文档要有对应用例）：链接被手工替换成真实目录、Windows 上无符号链接权限、同一 slug 在不同 target 指向不同版本（允许，属预期）、source 网络不可达（离线模式：只用缓存 catalog）、catalog 自身损坏（fallback 到仓库内直接扫 SKILL.md）。

---

## 9. 命令面（v1 完整清单）

命名遵循 `heron <domain> <verb>`，动词统一为 `add/rm/list/get/set`。

### 发现与收拢
```
heron scan    [--roots <path>] [--json]           # 扫描建索引，输出统计
heron list    [--target <t>] [--source <s>] [--unmanaged]
heron info    <slug>                              # 版本、来源、部署点、是否有本地改动
heron search  <kw>                                # 本地 + 远端一起搜
heron adopt   [<slug>|--all-detected] [--into <target>]   # ★ 收拢：迁入 store，原位留链接
heron doctor                                      # 体检：重复/冲突/嵌套/坏链/缺版本号
```

`adopt` 是本项目对 P1/P2 的正面回答：它把已有散落拷贝纳入管理，而不是要求用户重装。**这是决定工具有人用还是没人用的那一条命令。**

### 分发
```
heron use     <slug> --on <target>[,<target>…]      # 部署到指定 target
heron use     <slug> --everywhere                   # ★ 按 clients.toml 扇出到每个客户端各自的扫描路径
heron unuse   <slug> [--on <target>|--everywhere]   # 只断链接，store 保留
heron targets list | add <name> <path> | set-default <name>
heron link    <target>                              # 修复坏链
heron verify  [<slug>]                              # ★ 可见性校验：是否被某客户端扫到 / 是否被优先级压制
heron usage                                         # 各 skill 的 description token 成本 + 触发统计
```

`--everywhere` 是这条命令线的价值所在：用户不必知道"Claude 不扫 .agents"这种细节，heron 查表决定往哪落。  
`verify` 把 §3 第三条约束变成可执行动作 —— 它检查的不是文件在不在，而是**客户端到底看不看得见**：

* `visible` —— 落在该客户端扫描路径内且未被压制；
* `shadowed-by-plugin:<name>` —— 同名插件胜出，本条永不生效（如你实测的 `code-review`）；
* `not-scanned` —— 落点不在任何已登记客户端的扫描路径上（白装）；
* `needs-session-restart` —— 链接正确但当前会话尚未重新扫描。

`usage` 回应的是全量挂载的真实代价：45 个 skill 的 description 每次进上下文。默认策略因此定为 **A（按需挑几个）而非 B（全挂）**，`--everywhere` 只在用户明确要时提供，并在执行前打印预计 token 开销。

### 远程与安装
```
heron source add <url> [--id] [--ref] [--prefix] [--trust]
heron source list / remove / update [--all]
heron install  <slug>[@version] [--from <source-id>] [--on <target>…]
heron uninstall <slug> [--purge]                   # 默认保留 store 内容与 db 历史
```

### 升级
```
heron outdated [--json]
heron upgrade  [slug…] [--all] [--dry-run] [--to <ver>]
heron rollback <slug> --to <version>
heron prune    [--keep <n>]
```

### 编辑与协作
```
heron new      <slug> [--template <t>] [--editor]  # 生成合规骨架（含 metadata.version）
heron edit     <slug>                               # 打开 store 内副本
heron fork     <slug> [--as <new-slug>]             # 脱离上游追踪，转本地自主
heron diff     <slug> [--against <version>]         # 本地改动 / 两版本差异
heron publish  [--source <registry>]                # 从 store 反向生成 catalog 条目
heron export   [--format lock|json|toml] > heron.lock   # 团队共享：锁定版本清单
heron import   heron.lock                           # 按锁文件复现整套 skill
```

`export/import` 就是团队协作的答案：项目里放一份 `heron.lock`，新人 clone 后一条命令拿到同样的 skill 集合。

### 系统
```
heron config  get/set/list
heron cache  clean
heron log    [--follow]
heron completion bash|zsh|fish|powershell
```

### 交互原则

* 任何会改动 ≥2 个 target 的操作默认 `--dry-run` 打印计划树，`--yes` 跳过。
* 破坏性动作（prune/uninstall --purge/overwrite）必须先展示将受影响的真实路径列表。
* 全程幂等：重复 `use` 同一 skill 不产生第二份链接。

---

## 10. 跨平台细节（Windows 是难点，单列）

| 议题 | macOS / Linux | Windows |
|---|---|---|
| 用户数据目录 | `~/Library/Application Support/heron` | `%APPDATA%\heron`（走 `dirs` crate，不自拼） |
| 链接机制 | symlink（`std::os::unix`） | **优先 junction**（目录联接，普通权限即可）；目录→junction，文件→硬链接；符号链接仅在开发者模式/管理员下作为兜底 |
| 长路径 | 无碍 | 启用 `\\?\` 前缀 + 建议 `LongPathsEnabled`，否则深层 skill 目录（`references/…`）易撞 260 上限 |
| 行尾 | LF | 保守：不改写文本文件行尾，仅在 `.gitattributes` 层面提示；frontmatter 解析容忍 CRLF |
| 保留名 | 无碍 | 拒绝 `CON/NUL/aux.cs 之类` 命名的 slug，`doctor` 报错 |
| 权限 | 普通用户即可 | 不要求管理员是硬指标 → 这就是为什么选 junction 而非 symlink |
| 换行/编码 | UTF-8 | 强制 UTF-8 读写，`SKILL.md` 中文描述不会花屏 |
| 大小写 | 区分 | NTFS 默认不区分 → slug 唯一性检查用"忽略大小写"口径，防 `WECHAT-MP` 与 `wechat-mp` 双份 |
| 杀软/OneDrive | — | 文档提醒：把 store 排除在 OneDrive 同步外，否则链接会被云占位符化 |

结论：**junction 方案让 Windows 用户零权限负担**，这是选型阶段就要定死的事，不能留到实现后期补救。

---

## 11. 架构分层

```
heron/
├── crates/
│   ├── heron-core/          # 领域模型：Skill/Slug/Version/Target/Link，纯逻辑无 IO
│   ├── heron-fs/            # 扫描器、frontmatter 解析、规范化哈希、跨平台 link/junction
│   ├── heron-store/         # store 读写、版本 GC、db 仓储层（rusqlite）
│   ├── heron-git/           # bare clone、浅取、tag/SHA 解析、catalog 拉取
│   ├── heron-registry/      # source 抽象（registry/repo/skill 三型统一接口）
│   ├── heron-platform/      # 路径/编码/权限的平台差异收敛点（唯一允许 #[cfg(windows)] 的地方）
│   └── heron-cli/           # clap 命令面、输出渲染、交互确认
└── tests/
    ├── fixtures/            # 假 skill 仓库、病态嵌套样本、无版本号样本
    └── cases/               # adopt/use/upgrade/rollback 端到端
```

依赖方向单向向下：`cli → registry/store/fs/git → core ← platform`。  
`heron-core` 不碰 IO，保证升级算法可被单元测试穷举。

### 索引 schema（最小集）

```sql
client(id TEXT PK, display_name, scan_paths JSON, supports_links INTEGER,
       priority INTEGER, restart_required INTEGER, verified_on TEXT)   -- 兼容性矩阵，数据驱动
skill(slug TEXT PK, name TEXT, default_target_kind TEXT)
version(slug, version, sha256, commit, source_id, path_in_source, downloaded_at, PRIMARY KEY(slug,version))
installed(version_slug, version, content_hash, adopted_from TEXT NULL, modified INTEGER)
target(id TEXT PK, kind TEXT, client_id TEXT REFERENCES client, scope TEXT, path TEXT, is_default INTEGER)
link(target_id, slug, name, link_path TEXT, mode TEXT, broken INTEGER)
visibility(link_id, client_id, state TEXT, shadowed_by TEXT NULL)      -- verify 的落库结果
event(ts, op, slug, detail JSON)            -- 审计与回滚依据
```

`modified` 字段是 §8 安全升级的前提：`content_hash` 记录落地时的哈希，之后重算比对即可判定"用户手改过"。  
`client.scan_paths` 用 JSON 存多路径（一个客户端可能扫三处），`visibility` 表让 `verify` 可离线复现而不必每次真跑客户端探测。

---

## 12. 分期计划

| 里程碑 | 内容 | 完成判据 |
|---|---|---|
| **M0 地基** | 跨平台构建 + `platform` 层 + `clients.toml` 矩阵 + scan/list/db | 本机 537 个 SKILL.md 全部索引进来，误报 <5%；Claude/Codex/agents 三类 target 的扫描路径判定与实测一致 |
| **M1 收拢** | `doctor` + `adopt` + `use/unuse/targets/verify` | `.claude/.codex/.agents` 三处重复 skill 归一；重启会话后 Claude 通过链接加载成功，且 `code-review` 这类被插件压制的条目在 `verify` 里明确报 `shadowed-by-plugin` 而不是假装成功 |
| **M2 远程** | source 三型 + `install/search/outdated/upgrade/rollback` | 从默认仓库装→升级到新版本→回滚，全链路无人工干预 |
| **M3 协作** | `new/fork/diff/export/import/publish` | 项目内 `heron.lock` 能被另一台机器复现 |
| **M4 打磨** | completion / TUI / watch 自动刷新 / Linux CI | Windows 真机通过全量测试 |

建议 M0–M2 为 v1.0 发布线，M3 起进 beta。

---

## 13. 风险与对策

| 风险 | 影响 | 对策 |
|---|---|---|
| ~~Claude/Codex 不认 symlink 版 skill~~ **已排除** | — | 实测：经软链加载 `find-skills` 正常，Codex 对 `.agents` 软链同样识别。链接模型成立，无需实体副本兜底（`mode` 字段保留但不作为主路径） |
| **客户端扫描路径/优先级随版本变动** —— 今天 Claude 不扫 `.agents`，明天可能扫；插件优先级规则也可能改 | 高（`use --everywhere` 落点错误 → 静默白装） | 兼容性矩阵数据化为 `clients.toml`（§3.1），可随二进制更新、也可用户覆盖；每次 `verify` 以真实文件系统状态复核而非信任表；把"表过期"当作常态而非异常 |
| **同名被插件压制导致"装了但无效"，且失败无声** | 中（用户以为生效了） | `visibility.state = shadowed-by-plugin` 显式报错 + `doctor` 汇总所有 shadowed 条目；绝不静默删除那份被压制的链接 |
| **新建链接后当前会话看不到**（需重启会话） | 低但易误判为 bug | `use` 成功后固定提示 "restart session to pick up N new skills"；`verify` 返回 `needs-session-restart` 区分"永远不会生效"和"现在还没生效" |
| 全量挂载的 description token 开销（45 个 skill 每会话都进上下文） | 中（成本与触发准确率双降） | 默认按需 `use`；`--everywhere` 执行前打印预估 token；`usage` 长期跟踪哪些从未被触发 |
| 用户已有手工 symlink 被覆盖 | 信任崩塌 | `adopt` 前先 `--dry-run` 快照；`unmanaged` 条目永不改写（§3） |
| Windows 无符号链接权限 | 半残 | junction 为主方案（§10），CI 加 Windows runner 常态化 |
| 上游 skill 无版本号 | 升级无从判断 | 强制 `0.0.0+sha` 兜底 + `doctor` 持续提示补 `metadata.version` + `publish` 自动生成 |
| 全盘扫描慢/噪音大（worktree、vendor、编辑器扩展） | 体验差 | 默认 exclude 名单（§6.1）+ 增量 mtime 索引 + `--roots` 白名单 |
| 与 Claude plugin/marketplace 双头管理 | 冲突互相覆盖 | 检测到 plugin 管理的 skill 即标记 `owned-by-plugin`，只读不接管 |
| 一次升级打挂正在用的项目 | 数据风险 | 版本不可变 + 指针切换 + 事务回滚 + `event` 审计（§8） |

---

## 14. 开放问题（请评审时拍板）

1. **语言**：Rust（推荐）还是 Bun/TS？取决于团队长期维护者是谁。
2. **默认远程仓库**是否由我们自建（需要有人持续维护 catalog），还是 v1 直接把 Anthropic 官方 skills 仓库设为默认、自建 registry 推到 M3？
3. **marketplace 兼容深度**：是否需要读写 `~/.claude/plugins/known_marketplaces.json`（与官方 plugin 机制互通），还是只读不写？
4. **skill 内容是否允许加密/私有化**（团队场景可能有商业秘密），影响 store 是否引入可选 Keychain/DPAPI。
5. 病态嵌套（`skills/.agents/skills`）**要不要自动修**，还是只报告让人手工处理？
6. 项目级 target 的 `.claude/skills` 是否应纳入 Git 提交（推荐纳入 lock 而排除实体文件），需要在文档里给出团队规约建议。
7. **`~/.agents/skills` 的定位**：它已是 `npx skills` CLI 的通用 agent 目录、且是你本机 skill 最集中的一处（45 个）。heron 是把它当**默认 store 的物理位置**（复用现有惯例、Codex 直接可扫），还是仅当一个普通 target、store 另立在家用数据目录？前者迁移成本低但与 `npx skills` 争所有权，后者边界干净但 Claude 侧必须靠扇出链接。**这条影响 §5 磁盘布局，建议在写实现文档前定。**
8. **同名冲突策略要按客户端分别定**：`code-review` 的现实是插件赢、agent 目录那份永远无效。v1 该 (a) 拒绝安装同名到会被压制的位置，(b) 照装但报 shadowed，还是 (c) 提供 `--force-unique` 自动改名（如 `code-review-agents`）？

---

## 15. 验收标准（v1.0 DoD）

* 在一台装有 Claude + Codex 且 skill 散落 ≥3 处的真实机器上，`heron scan && heron adopt --all && heron use <slug> --everywhere` 三步内完成归一；**重启会话后**两个客户端均能实际加载该 skill（以客户端自身可见为准，不以文件存在为准）。
* `heron verify` 对"被插件压制""落点不被扫描""需重启"三种情形给出可区分的正确结论，无一例静默成功。
* `heron outdated --json` 在有更新的远端仓库上给出准确列表，误报为 0。
* `heron upgrade --all` 后可 `heron rollback <slug> --to <old>` 恢复，且期间任一中断（Ctrl-C / 断网）不留半态。
* Windows 11 普通用户（无管理员、无开发者模式）全流程跑通。
* 一个项目通过 `heron.lock` 在两台不同 OS 的机器上复现同一套 skill。
