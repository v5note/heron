# 苍鹭 Heron · 设计文档 v2

> **文档编号**：heron-DESIGN-002  
> **状态**：评审通过（v2.2，Q2–Q6 决议已落定，可转入《实现文档》）  
> **日期**：2026-11-19  
> **作者**：lead（AI 研发）  
> **取代**：旧版设计稿（已归档为 `docs/legacy/DESIGN-v0.2.md`，仅作历史参考）  
> **后续**：评审通过后产出《实现文档》（`docs/IMPLEMENTATION-heron-v1.md`），再按里程碑拆票

---

## 0. 阅读指引

| 角色 | 建议先读 |
|------|----------|
| 决策者 / 评审 | §1 背景 → §3 目标 → §4 关键决策 → §16 开放问题 |
| 产品 / 交互 | §2 用户故事 → §8 功能设计 → §12 界面设计 |
| 研发 | §5 架构 → §6 技术选型 → §7 数据模型 → §10 核心机制（重点 10.1 / 10.2 / 10.11–10.15） → §11 命令面 |

---

## 1. 背景与问题

### 1.1 一句话定位

**Heron（苍鹭）是一款跨平台（macOS / Windows）桌面应用**：把你电脑上散落在各处的 Agent Skill 统一收拢成"一份受版本管理的副本"，再按你的意愿分发到各客户端与各项目，并让"升级"变成一次点击或一条命令。

> 苍鹭静立一处，注视整片水面。Heron 之于 Skill，亦是如此。

### 1.2 问题描述

Skill 已经成为 AI 编码助手的核心资产：既承载个人方法论，又包含团队协作流程，还引用了大量脚本与参考资料。但它的**存储形态**仍停留在"复制粘贴到某个隐藏文件夹"的阶段，于是产生了系统性痛点：

| 编号 | 痛点 | 成因 |
|------|------|------|
| **P1** | **散落**：Skill 分散在 `~/.claude/skills`、`~/.codex/skills`、`~/.agents/skills`、`~/.cursor/skills` 以及 N 个项目各自的 `.claude/.codex/.agents` 下 | 每个客户端各自定义扫描路径；没有统一注册表 |
| **P2** | **重复**：同一个 Skill 有多份物理拷贝，改一处其余仍是旧的 | 手工复制是当下唯一的分发手段 |
| **P3** | **无法判断新旧**：不知道本地这份和上游差多少，升级只能整目录覆盖 | 多数 Skill 没有版本号，来源信息也不存在 |
| **P4** | **升级不可回滚**：覆盖即丢失，改过的内容无处找回 | 没有版本库与审计 |
| **P5** | **装了不生效**：文件在，但客户端扫描路径不同或同名被压制，于是静默无效 | 各客户端扫描路径 / 优先级规则不透明 |
| **P6** | **团队协作漂移**：项目内 `.claude/skills` 各人各改 | 没有锁文件、没有来源绑定 |
| **P7** | **补丁必须手工重打**：本地对 Skill 做了少量定制，每次升级都要重新 patch | 没有"上游 / 本地 / 部署"三层模型 |
| **P8** | **来源管理原始**：作者更新了 GitHub 仓库，只能靠"碰巧看到"才知道 | 没有远程来源与更新检测 |

### 1.3 本机现状：一个真实的样本

以下数据来自 2026-11-19 对作者 macOS 机器的实测，作为设计基线与验收样本：

| 观测 | 实测结果 |
|------|----------|
| 全局 Skill 目录 | `~/.claude/skills`（2 项）、`~/.codex/skills`（3 项）、`~/.agents/skills`（**45 项**）、`~/.cursor/skills-cursor`（≥10 项） |
| 现有"分发"手段 | 手工软链：`~/.claude/skills/find-skills` → `../../.agents/skills/find-skills` |
| 现有"同步"手段 | 手写脚本 `~/.agents/sync_skills.py`：把 `~/.agents/skills` 生成指针型 `SKILL.md` 到 `~/.workbuddy/skills/`；原因记录为"**部分扫描器对 symlink 目录的 dirent.isDirectory() 判定为 false**" |
| 现有"搜索"手段 | 手写脚本 `~/.agents/find_skill.py`：按关键词在 45 个 Skill 的名字 / 描述 / 正文里做模糊检索 |
| 现有"锁" | `~/.agents/.skill-lock.json`、`~/.agents/skills/skills-lock.json`（`npx skills` 写入，记录 source / skillPath / 内容哈希） |
| 版本号情况 | 部分 Skill 有 `metadata.version`（如 `huashu-design: "3.0.0"`、`reverse-engineer-anything: "42"`），大量 Skill 完全没有 |

**这张表有三点启示，直接决定了本设计：**

1. 用户已经在**自己写工具**来弥补缺口（同步、搜索、锁），说明需求真实且强；但这些脚本各管一摊、互不知情，本身就是新的碎片。Heron 要替换的**不是某一个脚本，而是这一类脚本**。
2. 用户已经踩过**链接兼容性**的坑，并明确记录了"不可见"的失败模式。因此 Heron 必须把"**装得上**"与"**客户端看得见**"显式区分，并提供多种投影方式兜底。
3. `npx skills` 已经形成了 `skills-lock.json` 生态，**必须兼容而不是另起炉灶**，否则用户的既有资产无法被收拢。

### 1.4 现有方案的不足

| 方案 | 能做到 | 做不到 |
|------|--------|--------|
| 手动 `cp -r` | 简单 | 必然重复、必然漂移（P2/P6） |
| 手动 `ln -s` | 单点更新 | 跨客户端路径各异、Windows 权限受限、无回滚（P1/P5） |
| `npx skills` | 从 GitHub 安装到 `.agents/skills`，带 lock | 只覆盖 `.agents` 一个落点；无本地定制管理；无多落点投影；无图形界面 |
| 各家 plugin / marketplace | 官方分发渠道 | 各客户端自成体系，**互不相通**（P1/P8） |
| Git submodule | 版本可追溯 | 必须侵入目标仓库；Windows 无符号权限时不可用；对非 git 项目完全不适用 |
| 上述零散脚本 | 解决单点 | 各管一摊，不能形成统一视图 |

---

## 2. 用户与使用场景

### 2.1 目标用户

| 画像 | 特征 | 最关心的点 |
|------|------|-----------|
| **重度个人用户**（首要） | 同时使用 2–4 个 AI 编码客户端，自建 20+ Skill，会写小脚本 | 全局视图、一键同步、升级不丢改动 |
| **团队协作成员** | 项目里需要统一的 Skill 集合 | 锁文件、来源可追溯、新人一条命令就位 |
| **Skill 作者** | 持续迭代并发布 Skill 到 Git 仓库 | 发布流程、版本号、变更记录 |
| **初学者** | 只会点图形界面 | 不出错、看得懂、能回退 |

### 2.2 核心用户故事

| ID | 作为… | 我希望… | 以便… | 验收口径 |
|----|-------|---------|-------|----------|
| US-1 | 个人用户 | 打开 Heron 就看到全部 Skill 及它们在各客户端的安装状态 | 不再 `ls` 十几个目录 | 首屏列出所有"已纳管 / 未纳管"Skill |
| US-2 | 个人用户 | 把散落的同一 Skill 多份拷贝**收拢为一份**并自动重新分发 | 改一处处处生效 | 收拢后只剩一份实体，原位置变为投影或备份 |
| US-3 | 个人用户 | 给一个 Skill 选择"哪些客户端、哪些项目"启用 | 精确控制上下文体积 | 开关即生效，并能提示"需重启会话" |
| US-4 | 个人用户 | 一键检查哪些 Skill 有上游更新，并看到**我能合并的差异** | 安全升级 | 差异预览 + 三方合并结果 + 回滚 |
| US-5 | 个人用户 | 直接粘贴一个 GitHub 地址（仓库 / 子目录 / 单 Skill）就装上 | 不必了解仓库结构 | URL 解析成功率高，失败时给出可理解原因 |
| US-6 | 个人用户 | 添加自己的 Git 仓库（GitHub / GitLab / Gitee / 私有）作为来源 | 团队内部分发 | 支持 registry 仓库与代码仓库两种形态 |
| US-7 | 个人用户 | 修改某个已安装 Skill 而不被后续升级覆盖 | 保留本地定制 | 本地修改单独记录，升级自动三方合并，冲突可手工解决 |
| US-8 | 团队用户 | 项目里放一份锁文件，别人 clone 后一条命令就位 | 消除团队漂移 | `heron.lock` 导出/导入 + `heron sync` |
| US-9 | 个人用户 | 知道某个 Skill "为什么没生效" | 不再靠猜排错 | 体检给出"未扫描 / 被压制 / 需重启 / 投影失败"等具体结论 |
| US-10 | 所有用户 | 在 Windows 上无需管理员权限即可使用全部功能 | 降低使用门槛 | 普通用户账户全流程通过 |

---

## 3. 目标与非目标

### 3.1 阶段化目标

| 阶段 | 平台 | 形态 | 内容 |
|------|------|------|------|
| **Phase 1（本期）** | macOS + Windows | 桌面 GUI + 内嵌 CLI | 发现、收拢、管理、分发、远程安装、升级/回滚、体检、锁文件 |
| **Phase 2** | 增加 Linux | 同上 | 同等能力；架构上自 Phase 1 起即不允许平台特化假设 |
| **Phase 3** | 全平台 | 独立 CLI + CI 集成 + 客户端原生扫描支持 | 无 GUI 环境、流水线；随生态演进跟进 |

### 3.2 Phase 1 目标

* **G1 发现**：自动扫描本机所有已知位置，识别 Skill、宿主客户端、纳管状态，建立索引。
* **G2 收拢（Adopt）**：把散落拷贝纳入受管 Store，**原位不留破坏**，同内容自动去重。
* **G3 分发（Project）**：从 Store 单向投影到任意目标，支持 **copy / symlink / junction / pointer** 四种方式。
* **G4 来源（Source）**：内置默认市场；支持任意 Git 地址；支持 registry 仓库 / 代码仓库 / 单 Skill 子目录 / 本地路径。
* **G5 升级（Upgrade）**：检测上游更新；本地改动经**三方合并**保留；失败自动回滚。
* **G6 体检（Doctor）**：重复、冲突、坏链、未扫描、不合规 frontmatter、病态嵌套、丢失来源。
* **G7 跨平台**：macOS + Windows 一等公民；Linux 架构就绪。
* **G8 团队**：`heron.lock` 导出/导入，可复现整套 Skill 清单。

### 3.3 非目标（明确不做）

| 不做 | 理由 |
|------|------|
| 管理 MCP server、subagent、hook、CLAUDE.md | 只做 Skill，避免范围蔓延；但需**识别**它们以免误判/误删 |
| 改造或劫持客户端加载机制 | Heron 只产出客户端本来就认识的目录布局 |
| 云账号 / 云同步 | Git 即同步；引入账号会带来隐私与合规负担 |
| 商店运营（评分、推荐、付费） | Phase 1 只做"来源"，不做"生态运营" |
| 远程执行 Skill 中的脚本 | 安全边界；Heron 永不自动执行任何 Skill 代码 |
| 接管 `~/.claude/plugins` 全量生态 | 只读兼容（识别归属），不做插件级别接管 |
| Linux / 其它客户端首发支持 | Phase 2 起 |

---

## 4. 关键决策（ADR 摘要）

> 每条决策给出：**决策 / 理由 / 后果**。详细论证见对应章节。

### ADR-1　产品形态：桌面 GUI 为主，CLI 内核同源

* **决策**：Phase 1 交付**桌面应用**（GUI 外壳 + Rust 内核），同一份内核同时提供 `heron` 命令行（内置命令面板 + 独立可执行文件）。
* **理由**：用户要"管理器"，图形化视图是解决 P1（看不见）最直接的手段；而 Agent 工具生态天然要求可被脚本调用，因此内核必须是 CLI 可驱动的。
* **后果**：内核先做成稳定的库 + CLI，GUI 是薄壳；所有功能都能在无 GUI 环境测试。

### ADR-2　技术栈：Rust 内核 + Tauri 2

* **决策**：核心逻辑 Rust；GUI 采用 **Tauri 2**（系统 WebView）；前端 **Vue 3 + TypeScript + Vite + Pinia + Vue Router + Tailwind CSS**。
* **理由**：
  1. 单一跨平台构建体系，**Windows 无需安装 Node/Python 等运行时**，安装包约 5–15 MB（对比 Electron 通常 80 MB+）。
  2. 内核本质是"文件系统 + Git + SQLite 索引"，原生实现对 Windows reparse point 与长路径的控制更精准。
  3. 同一套 Rust 代码可直接产出 `heron-cli`，无需二次开发。
* **后果**：Windows 需要 WebView2（Win10 1803+ / Win11 默认内置）；macOS 使用系统 WKWebView。见 §10.5。

### ADR-3　存储模型：受管 Store 内实体化，向目标**单向投影**

* **决策**：Skill 实体唯一存放在受管 Store 中；目标位置（`~/.claude/skills/` 等）只接受"投影"，逐条可在 copy / symlink / junction / pointer 之间切换。
* **理由**：解决 P2（重复）与 P7（改动丢失）；单向投影保证"Store 是唯一真相源"，避免多落点互相修改导致漂移。
* **备选**：把 `~/.agents/skills` 直接当作 Store —— 迁移成本低，但与 `npx skills` 争所有权，且无法保留版本。
* **后果**：需要"漂移检测"（目标被外部改成实体目录或被手工修改时报告而非静默覆盖）；需要"未纳管"概念。

### ADR-4　Source：统一抽象 + 两种 catalog 格式 + 兜底扫描

* **决策**：来源统一抽象为 `GitSource`（含 ref / subpath / skill 过滤）与 `LocalSource`。GitSource 支持四种 URL 形态（HTTPS / SSH / `owner/repo` 简写 / 子目录深链），catalog 优先读 **`.claude-plugin/marketplace.json`**，其次读 **`heron.source.toml`**，都没有则**直接扫描 `SKILL.md`**。
* **理由**：降低迁移门槛（用户已有的 marketplace 仓库立刻可用）；避免自建生态的冷启动问题。
* **后果**：需要维护两套 catalog 解析 + 一个兜底扫描器；解析器必须容错。

### ADR-5　版本与升级：Git 作为唯一 VCS，本地改动独立成分支

* **决策**：每个受管 Skill 拥有一个**独立 Git 仓库**（`store/<slug>/.git`）：上游镜像为分支 `upstream`，本地修改提交到分支 `local`，基线记录在 `store/<slug>/heron.skill.json`；升级时基于基线做**三方合并**，成功才切换并刷新投影，失败则回滚。
* **理由**：
  1. 内容哈希无法表达"哪些行是本地改的"，只有 VCS 能提供可合并、可回滚、可审计的路径（解决 P3/P4/P7）。
  2. 无需任何版本号约定即可判断新旧（比较 commit 与内容哈希）。
  3. 对用户可见、可救援（随时 `git log`）。
* **备选**：纯内容哈希 + 快照目录 —— 实现简单，但不能三方合并、回滚粒度粗。
* **后果**：Store 内多出 `.git` 目录，必须在防嵌套扫描与体积管理中显式处理。

### ADR-6　Git 操作：系统 `git` 子进程为主，`gix` 只读为辅

* **决策**：克隆 / 拉取等"写"操作调用系统 `git`；读取（log / 分支 / 差异）可用 `gix` 库。
* **理由**：系统已有 `git` 的概率极高（Skill 大多来自 GitHub），成熟实现能正确处理凭据、代理、SSH、LFS 等边界。
* **后果**：启动时做 preflight 检测 `git`，缺失时**引导用户安装**（D7，见 §10.4 / §10.12）；不提供静默降级。

### ADR-7　默认投影：macOS 用符号链接，Windows 用目录联接（junction）

* **决策**：默认 `mode=auto` —— macOS → `symlink`；Windows → **`junction`**（目录联接，普通权限可创建）。另提供 `copy`（全平台兜底）与 `pointer`（指针文件，应对不识别链接的扫描器）。
* **理由**：Windows 创建符号链接需要管理员或开发者模式，junction 零权限负担。
* **后果**：**必须**正确处理 Windows 上 junction 的删除语义（见 §10.2，头号实现陷阱）。

### ADR-8　本地数据目录：遵循各家规范；Store 位于 Heron 自有目录 `~/.heron/skills`，`~/.agents` 仅作映射点

* **决策**：使用 `directories` 约定解析用户数据 / 缓存 / 日志目录；Bundle identifier 取自有域名 `v5ai.xin` 的反向 DNS（`xin.v5ai` + 应用名 `heron` → `xin.v5ai.heron`），**不由仓库或 GitHub 账号派生**；Store 位于用户主目录下的 `~/.heron/skills`（Windows `%USERPROFILE%\.heron\skills`），数据目录（macOS `~/Library/Application Support/xin.v5ai.heron`，Windows `%LOCALAPPDATA%\v5ai\heron\data`）只放索引、缓存、备份、日志。
* **理由**：Store 是全部 Skill 实体唯一的存放地，放在 Heron 自己的目录里既不依赖任何第三方客户端的版本行为，也不会落在扫描路径中（不会自己扫自己）；与 `npx skills` 拥有的 `~/.agents` 解耦；Windows 上索引与备份留在 `%LOCALAPPDATA%`，不参与 OneDrive 漫游，避免链接被云占位符化。标识按域名而非仓库派生，是因为域名代表产品的长期归属，而仓库位置随时可迁（换账号、换托管平台）——把标识绑在仓库上，一次迁移就会改变用户机器上的数据目录。因此更新源在 GitHub 账号 `v5note` 下（`https://github.com/v5note/heron/releases/latest/download/latest.json`）与标识 `xin.v5ai` 不一致，是有意为之：前者是发布渠道，后者是应用身份。
* **后果**：用户历史资产集中在 `~/.agents`，**收拢流程（Adopt）必须是一等公民**，否则用户会感到"东西被搬走了"。指定 `--data-dir` 时该目录拥有 Heron 写入的一切，Store 也随之落到 `<data-dir>/.heron/skills`。
* **投影目标（D10）**：Store 本身**不是**投影目标，也没有"默认目标"——`heron project <slug>` 必须用 `--on` 显式指定客户端目标。`~/.agents/skills` 是普通的可选映射点，按需开启且与 `npx skills` 共享所有权。详见 §10.15。

### ADR-9　CLI 与 GUI 共享一套状态：单实例 + 本地 IPC

* **决策**：GUI 启动即成为本地守护进程（仅绑定 `127.0.0.1` + 随机令牌）；CLI 若发现守护进程在运行则通过 IPC 委托，否则自行加锁执行。
* **理由**：避免 GUI 与 CLI 并发写索引；为 Phase 3 的独立 CLI 与外部集成预留统一入口。
* **后果**：需要单实例锁与版本化 IPC 协议（见 §10.7）。

### ADR-10　兼容既有生态：读写 `skills-lock.json`

* **决策**：收拢时识别 `skills-lock.json` / `.skill-lock.json` 中的 source / skillPath / hash，自动回填来源；导入锁文件时应用同样的兼容层。
* **理由**：用户机上的 45 个 Skill 大多由 `npx skills` 安装，来源信息已经存在，不应丢弃。
* **后果**：需同时支持两种 lock 版本格式（`version: 1` 与 `version: 3`，字段名不同）。

---

## 5. 总体架构

### 5.1 分层视图

```text
┌──────────────────────────────────────────────────────────────┐
│ 表现层                                                       │
│  ┌───────────────────────┐   ┌────────────────────────┐      │
│  │ heron-app (Tauri 2)   │   │ heron-cli (clap)       │      │
│  │ Vue 3 + TS 前端       │   │ 亦可被 GUI 内嵌调用    │      │
│  └──────────┬────────────┘   └───────────┬────────────┘      │
│             │ Tauri IPC (commands/events)│ 本地 IPC 委托      │
├─────────────▼────────────────────────────▼───────────────────┤
│ 编排层 heron-service                                         │
│  scan · adopt · install · project · upgrade · doctor · lock  │
│  daemon（单实例、文件监听、后台任务）                          │
├──────────────────────────────────────────────────────────────┤
│ 能力层（crates）                                              │
│  heron-store(仓储)  heron-projection(投影)  heron-git(远端)   │
│  heron-source(来源)  heron-vcs(版本/合并)   heron-scan(扫描)  │
├──────────────────────────────────────────────────────────────┤
│ 基础层 heron-core（领域模型，无 IO） + heron-platform（OS）    │
├──────────────────────────────────────────────────────────────┤
│ 持久层 SQLite(索引) · Git(内容版本) · TOML/JSON(配置/锁)      │
│ ⇒ 外部世界：各客户端目录、远程 Git 仓库                        │
└──────────────────────────────────────────────────────────────┘
```

**依赖规则（强制）**：

* `heron-core` 依赖为零（不碰 IO、时间、网络）→ 命名、比较、冲突、语义化版本等纯逻辑可被穷举单测。
* `heron-platform` 是**唯一**允许出现 `#[cfg(windows)]` / `#[cfg(unix)]` 的 crate。
* 上层可依赖下层，下层不可反向依赖；GUI 不直接调用能力层，只通过 `heron-service` 暴露的用例接口。

### 5.2 组件职责

| 组件 | 职责 | 不做什么 |
|------|------|----------|
| `heron-core` | Slug、frontmatter 模型、SemVer、PinPolicy、ConflictPolicy、投影方式枚举、规范化哈希算法定义、错误码 | 任何 IO |
| `heron-platform` | 数据目录解析、长路径处理、链接/联接的创建与识别、reparse point 删除、文件锁、原子写、单实例 | 业务语义 |
| `heron-scan` | 受控遍历、`SKILL.md` 识别、frontmatter 解析、忽略规则、并行 | 写库决策 |
| `heron-store` | SQLite 仓储、事务、迁移、审计事件 | 投影与网络 |
| `heron-vcs` | 每 Skill Git 仓库初始化、提交、分支、三方合并、标签、垃圾回收 | 远端传输 |
| `heron-git` | 远端克隆/抓取/浅取缓存、凭据与代理透传、超时重试、离线降级 | 合并策略 |
| `heron-source` | URL 解析、catalog 解析、来源注册表 | 安装动作 |
| `heron-projection` | 投影的创建/校验/修复/移除、漂移检测、可见性判定 | 决定投影去哪 |
| `heron-service` | 用例编排、事务边界、进度事件、取消 | 具体 IO 细节 |
| `heron-app` | Tauri 命令、窗口/托盘、前端打包、自动更新 | 业务逻辑 |
| `heron-cli` | 参数解析、JSON 输出、退出码 | 业务逻辑 |

### 5.3 运行时形态

```text
                 ┌────────────── heron daemon（单实例）────────────────┐
  GUI 启动 ────▶ │ Tauri Window ⇄ Service ⇄ SQLite / FS / Git          │
  CLI 调用 ────▶ │        ↑                                          │
  外部集成  ────▶ │ IPC：仅 127.0.0.1 + 随机令牌（令牌写入 ipc.json）    │
                 └──────────────────────┬────────────────────────────┘
                                        │ notify 监听（防抖 1.5s）
                                        ▼
                    ~/.claude/skills、~/.agents/skills、项目目录…
```

* **无 GUI 时**：CLI 直接加载内核完成操作，结束后释放锁；GUI 下次启动时按"索引版本号"做增量对账。
* **有 GUI 时**：CLI 通过 IPC 转发（避免 SQLite 并发写冲突）；IPC 不可用时降级为"获取独占文件锁 → 直接操作"。

---

## 6. 技术选型

### 6.1 选型对比

| 维度 | Rust + Tauri 2 ✅ | Electron + Node | Go + Wails | Python + Qt |
|------|------------------|-----------------|------------|-------------|
| Windows 零运行时依赖 | ✅（WebView2 随系统） | ✅ 但体积大 | ✅（需 WebView2） | ❌ 需解释器 |
| 安装包体积 | 5–15 MB | 80–150 MB | 8–20 MB | 30 MB+ |
| 冷启动 | 快 | 中 | 快 | 慢 |
| 并发扫描 / 遍历性能 | 优 | 中 | 优 | 差 |
| Git 集成 | 子进程 + `gix` | 纯 JS 实现不成熟 | go-git 一般 | 一般 |
| SQLite | `rusqlite`（bundled） | 原生绑定 | 原生驱动 | 原生 |
| Windows reparse point 控制 | 优（可直接调 Win32） | 受限 | 一般 | 受限 |
| 团队门槛 | 中（需 Rust） | 低 | 中 | 低 |

### 6.2 最终选型

**Rust 内核 + Tauri 2 + Vue 3 / TypeScript 前端。**

关键依赖（版本以实现文档为准）：

| 领域 | 选型 | 备注 |
|------|------|------|
| CLI 解析 | `clap`（derive） | 子命令 + `--json` |
| GUI | `tauri` 2.x + `tauri-plugin-single-instance` / `-updater` / `-opener` | 系统托盘、二级实例转发 |
| 前端 | Vue 3 + TypeScript + Vite + Pinia + Vue Router + Tailwind CSS | 数据经 Tauri command 拉取；表格/虚拟滚动采用轻量方案 |
| 配置 | `serde` + `toml` / `serde_json` | 人类可编辑 |
| 索引 | `rusqlite`（bundled） | 零外部依赖 |
| Markdown / YAML | `pulldown-cmark`（预览）、`serde_yaml` 或 `saphyr`（frontmatter，需容忍非标准 YAML） | 见 §10.9 |
| 文件监听 | `notify` | 增量刷新 |
| 并发 | `rayon` / `tokio`（Tauri 异步运行时） | 扫描与网络分区 |
| Git | 系统 `git` 子进程 + `gix` 只读 | ADR-6 |
| 哈希 | `blake3` | 内容指纹 |
| 差异 | `similar` | 差异与三方合并兜底展示 |
| 路径 / 目录 | `camino`、`pathdiff`、`directories` | 跨平台路径 |
| 日志 | `tracing` + `tracing-appender` | 文件 + 结构化 |

### 6.3 跨平台适配点总表

| 议题 | macOS | Windows | Linux（Phase 2） |
|------|-------|---------|------------------|
| 数据目录 | `~/Library/Application Support/xin.v5ai.heron` | `%LOCALAPPDATA%\v5ai\heron\data` | `~/.local/share/heron` |
| 配置目录 | 同上 | `%APPDATA%\v5ai\heron\config` | `~/.config/heron` |
| 默认投影 | `symlink` | `junction` | `symlink` |
| 链接识别 | `symlink_metadata` | 需区分 **symlink 与 junction**（二者都被 `is_symlink()` 报告） | 同 macOS |
| 链接删除 | `remove_file` 安全 | **junction 必须先解除 reparse point**，否则可能连带删除目标内容 | — |
| 路径长度 | 几乎无限制 | 260 上限风险；需 `\\?\` 前缀 | 同 macOS |
| 大小写 | 默认区分 | 默认不区分 → 拒绝"仅大小写不同"的 slug 重名 | 区分 |
| 保留名 | 无 | 拒绝 `CON / PRN / AUX / NUL / COM1-9 / LPT1-9` 及尾随点、空格 | 无 |
| 非法字符 | `/` 与 NUL | `< > : " / \ | ? *` 及控制字符 | `/` |
| 文本编码 | UTF-8 | 强制 UTF-8，容忍 CRLF / BOM | UTF-8 |
| 可执行脚本 | `chmod +x` | `.ps1` / `.cmd` / `.py`，无执行位概念 | `chmod +x` |
| 长路径 API | — | 优先使用宽字符 Win32 API | — |

---

## 7. 数据模型

### 7.1 磁盘布局

```text
~/.heron/skills                    # Store：唯一真相源（Heron 自有，Windows %USERPROFILE%\.heron\skills）
│   └── <slug>/
│       ├── .git/                  # 每 Skill 独立 Git 仓库（ADR-5）
│       ├── heron.skill.json       # 本地元数据：基线、来源、投影推导信息
│       ├── SKILL.md
│       └── …                      # 其余 Skill 内容

<DATA_DIR>                         # macOS: ~/Library/Application Support/xin.v5ai.heron
│                                  # Win:   %LOCALAPPDATA%\v5ai\heron\data
├── heron.db                       # SQLite 索引（可从 store 重建）
├── config.toml                    # 用户配置（人类可编辑）
├── sources.toml                   # 来源注册表（人类可编辑）
├── targets.toml                   # 目标注册表（全局目标 + 项目目标）
├── clients/                       # 客户端兼容性矩阵（内置只读 + 用户覆盖）
│   └── overrides.toml
├── sources-cache/                 # 远端裸仓缓存（可安全删除重建）
│   └── <sha256-of-url>/
├── backups/                       # 收拢 / 覆盖前的安全备份
│   └── <timestamp>/<slug>@<location>/
├── trash/                         # 软删除区，可恢复
├── log/heron.log
└── ipc.json                       # daemon 端口与令牌（仅当前用户可读）
```

Store 不在数据目录里：数据目录不再有 `store/` 一项，实体一律在 `~/.heron/skills`（ADR-8）。指定 `--data-dir` 时该目录拥有 Heron 写入的一切，Store 也随之变为 `<data-dir>/.heron/skills`。

**关于 Store 内的 `.git`**：扫描器的忽略规则必须显式排除 `.git/**`，避免病态递归；Store 自身也不在任何客户端的扫描路径内，因此不存在"自己扫自己"。所有版本能力都以 Git 为基础；未安装 `git` 时按 D7 **引导安装**，不提供静默降级（见 §10.4）。

### 7.2 目录约定与投影规则

| 目标类型 | 全局路径（示例） | 项目路径（示例） | 默认投影 |
|----------|------------------|------------------|----------|
| `claude` | `~/.claude/skills/<name>` | `<proj>/.claude/skills/<name>` | auto |
| `agents` | `~/.agents/skills/<name>` | `<proj>/.agents/skills/<name>` | auto |
| `codex` | `~/.agents/skills/<name>`（跨客户端约定） | `<proj>/.agents/skills/<name>` | auto |
| `cursor` | `~/.cursor/skills/<name>` | `<proj>/.cursor/skills/<name>` | auto |
| `opencode` | `~/.config/opencode/skills/<name>` | `<proj>/.opencode/skills/<name>` | auto |
| `custom` | 用户自定义目录 | 用户自定义 | auto |

> Store `~/.heron/skills/<slug>` 不在此表内：它是被投影的**源**，不是投影目标（ADR-8 / D10）。

> 上表是**可热更配置**（`clients/*.toml`），不是硬编码。每项带 `verified_at` 与 `verified_by`（`spec` / `doc` / `probe`），GUI 展示证据链，避免把未验证的路径当真理（见 §15 风险 R1）。

**项目目标的发现方式（评审决议 D3：自动 + 手动并存）**：

| 方式 | 触发 | 说明 |
|------|------|------|
| 自动发现 | `heron scan` 命中含 `.claude/.agents/.codex/.cursor/.opencode` 且含 `SKILL.md` 的目录，或命中项目根标记（`.git` / `package.json` / `Cargo.toml` / `pyproject.toml` / `.sln`） | 自动登记到 `targets.toml` 的 `[[project]]`，状态为 `auto`；**不自动向其投影**，仅在界面"发现的项目"中列出供用户选择 |
| 手动登记 | 用户在"项目"页添加目录，或 CLI `heron targets add` | 状态为 `manual` |
| 自动结果可固化 | 用户把 `auto` 项目提升为受管 | 之后该项目随扫描持续跟踪，可绑定 `heron.lock` |

**Store 与映射点（评审决议 D10）**：

* **Store** `~/.heron/skills/`：全部 Skill 实体唯一的存放地（Heron 完全拥有，不依赖任何第三方客户端的版本行为，也不在任何客户端的扫描路径内）。它**不是**投影目标，因此没有"默认投影目标"这回事——`heron project <slug>` 必须用 `--on <target>` 显式点名要写入哪个客户端目录。
* **`~/.agents/skills` 仅作映射点**：它被当作"跨客户端共享入口"按需开启，Heron 不接管 `npx skills` 在该目录的所有权；开启时由 Heron 在该目录建立**指向 Store 的投影**，并在界面标注"此目录由 npx skills 与 Heron 共享"。
* 客户端自有目录（`.claude/skills` 等）是仅有的投影目标，用于"让某个客户端看到"的场景。

安全约束：自动发现得到的**项目级投影默认全部关闭**（项目 Skill 来自他人仓库，不可信）；启用需用户显式确认，符合 §9.4 的信任原则。

### 7.3 SQLite 索引 Schema（最小集）

```sql
PRAGMA journal_mode = WAL;

-- 客户端兼容性矩阵（随配置导入）
CREATE TABLE client (
  id               TEXT PRIMARY KEY,     -- claude / codex / agents / cursor / opencode / custom
  display_name     TEXT NOT NULL,
  global_paths     TEXT NOT NULL,        -- JSON array
  project_paths    TEXT NOT NULL,        -- JSON array（相对项目根）
  link_modes       TEXT NOT NULL,        -- JSON array
  priority         INTEGER NOT NULL DEFAULT 0,
  verified_at      TEXT,                 -- 证据时间
  verified_by      TEXT                  -- spec / doc / probe
);

-- Skill 身份：slug 全局唯一（大小写不敏感）
CREATE TABLE skill (
  slug             TEXT PRIMARY KEY,
  display_name     TEXT NOT NULL,
  description      TEXT NOT NULL DEFAULT '',
  tags             TEXT NOT NULL DEFAULT '[]',
  status           TEXT NOT NULL DEFAULT 'active',   -- active / trashed
  created_at       TEXT NOT NULL,
  updated_at       TEXT NOT NULL
);

-- 来源绑定（同一 skill 可有多条，如 fork）
CREATE TABLE source_binding (
  id               INTEGER PRIMARY KEY,
  slug             TEXT NOT NULL REFERENCES skill(slug) ON DELETE CASCADE,
  kind             TEXT NOT NULL,        -- git / local / adopted
  url              TEXT,                 -- 规范化 URL
  ref              TEXT,                 -- NULL 表示默认分支
  subpath          TEXT,
  pinned_commit    TEXT,
  remote_version   TEXT,
  content_hash     TEXT,
  is_primary       INTEGER NOT NULL DEFAULT 1,
  UNIQUE(slug, url, subpath)
);

-- 版本历史（落地为 Git tag）
CREATE TABLE version (
  slug             TEXT NOT NULL,
  version          TEXT NOT NULL,        -- semver 或 0.0.0+<shortsha>
  commit           TEXT NOT NULL,
  content_hash     TEXT NOT NULL,
  size_bytes       INTEGER NOT NULL DEFAULT 0,
  fetched_at       TEXT NOT NULL,
  origin           TEXT NOT NULL,        -- upstream / local / merge
  PRIMARY KEY (slug, version)
);

-- 目标（落地点）
CREATE TABLE target (
  id               TEXT PRIMARY KEY,     -- 'claude:global' / 'claude:project:<hash>'
  kind             TEXT NOT NULL REFERENCES client(id),
  scope            TEXT NOT NULL,        -- global / project
  project_path     TEXT,
  path             TEXT NOT NULL,        -- 最终 skills 目录
  enabled          INTEGER NOT NULL DEFAULT 1,
  is_default       INTEGER NOT NULL DEFAULT 0
);

-- 投影记录：一条 = 某个目标里的一个落点
CREATE TABLE projection (
  id               INTEGER PRIMARY KEY,
  slug             TEXT NOT NULL REFERENCES skill(slug),
  target_id        TEXT NOT NULL REFERENCES target(id) ON DELETE CASCADE,
  link_name        TEXT NOT NULL,        -- 落点目录名（可不同于 slug）
  path             TEXT NOT NULL,        -- 绝对路径
  mode             TEXT NOT NULL,        -- copy / symlink / junction / pointer
  version          TEXT,
  content_hash     TEXT,                 -- 落地时的内容指纹
  state            TEXT NOT NULL,        -- ok / broken / drifted / modified / shadowed
  created_at       TEXT NOT NULL,
  UNIQUE(target_id, link_name)
);

-- 来源注册表镜像（权威在 sources.toml）
CREATE TABLE source (
  id               TEXT PRIMARY KEY,
  name             TEXT NOT NULL,
  kind             TEXT NOT NULL,        -- git / local / market
  adapter          TEXT,                 -- market 时的适配器 id（见 §9.5）
  url              TEXT,
  ref              TEXT,
  subpath          TEXT,
  priority         INTEGER NOT NULL DEFAULT 100,
  trusted          INTEGER NOT NULL DEFAULT 0,
  enabled          INTEGER NOT NULL DEFAULT 1,
  last_synced_at   TEXT,
  last_commit      TEXT
);

-- 扫描发现的未纳管条目
CREATE TABLE detected (
  path             TEXT PRIMARY KEY,
  real_path        TEXT,                 -- 解析链接后的真实路径
  client_id        TEXT,
  scope            TEXT,
  project_path     TEXT,
  observed_slug    TEXT,
  content_hash     TEXT,
  frontmatter_ok   INTEGER,
  last_seen_at     TEXT NOT NULL,
  adopt_state      TEXT NOT NULL DEFAULT 'candidate'  -- candidate / ignored / adopted / conflict
);

-- 可见性判定缓存（verify 的结果）
CREATE TABLE visibility (
  projection_id    INTEGER PRIMARY KEY REFERENCES projection(id) ON DELETE CASCADE,
  client_id        TEXT NOT NULL,
  state            TEXT NOT NULL,        -- visible / not_scanned / shadowed / restart_required / broken
  shadowed_by      TEXT,
  checked_at       TEXT NOT NULL
);

-- 审计事件（所有写操作）
CREATE TABLE event (
  id               INTEGER PRIMARY KEY,
  ts               TEXT NOT NULL,
  actor            TEXT NOT NULL,        -- gui / cli
  op               TEXT NOT NULL,        -- adopt / install / project / upgrade / rollback / ...
  subject          TEXT,
  detail           TEXT,                 -- JSON
  undoable         INTEGER NOT NULL DEFAULT 0
);
```

### 7.4 配置文件

**config.toml**

```toml
[general]
data_dir_override = ""            # 允许用户迁移数据目录
language = "zh-CN"
default_projection = "auto"       # auto | copy | symlink | junction | pointer
conflict_policy = "ask"           # ask | keep-old | overwrite | side-by-side
auto_upgrade_check = "daily"      # off | launch | daily | weekly
telemetry = false                 # Phase 1 永不联网上报

[scan]
roots = []                        # 额外扫描根
max_depth = 8
max_dirs = 50000
follow_links = false              # 默认不跟随，防环
exclude = [
  "**/node_modules/**", "**/.git/**", "**/target/**", "**/dist/**",
  "**/.venv/**", "**/.codex/vendor_imports/**", "**/.vscode/extensions/**",
]

[upgrade]
pin = "semver"                    # exact | semver | none（见 §10.4）
auto_three_way_merge = true
conflict_resolution = "manual"    # manual | prefer-local | prefer-upstream
keep_versions = 10
prune_after_days = 90

[native_scan]
enabled = false                   # 客户端原生扫描注入，默认关闭（D6 / §10.11）
require_confirmation = true       # 每次写入都展示 diff 并请求确认
clients = []                      # 允许注入的客户端 id，空 = 全部禁止

[git]
binary = "git"                    # 可指定自定义 git 路径
network_timeout_secs = 60
shallow = true
proxy = ""                        # 空则继承系统环境
```

**sources.toml**

```toml
# —— 内置市场（首次启动写入，可在界面禁用/删除）——
[[source]]
id = "builtin-skills-sh"
name = "skills.sh"
kind = "market"
adapter = "skills_sh"
priority = 0
trusted = true
enabled = true

[[source]]
id = "builtin-clawhub"
name = "ClawHub"
kind = "market"
adapter = "clawhub"
priority = 1
trusted = true
enabled = true

[[source]]
id = "builtin-qoder"
name = "Qoder 能力市场"
kind = "market"
adapter = "qoder"
priority = 2
trusted = true
enabled = true           # B 档：引导式（见 §9.5）

[[source]]
id = "builtin-skillhub-cn"
name = "SkillHub 中国社区"
kind = "market"
adapter = "skillhub_cn"
priority = 3
trusted = true
enabled = true           # B 档：引导式

[[source]]
id = "builtin-skillsmp"
name = "SkillsMP"
kind = "market"
adapter = "skillsmp"
priority = 4
trusted = false
enabled = false          # B 档：反爬，默认关闭（评审决议 D1）

# —— 用户自定义来源 ——

[[source]]
id = "team-internal"
name = "团队内部市场"
kind = "git"
url = "git@gitlab.example.com:ai/skills.git"
ref = "stable"
priority = 10

[[source]]
id = "local-dev"
name = "本地开发中"
kind = "local"
path = "/Users/me/dev/my-skills"
priority = 100
```

### 7.5 锁文件 heron.lock（团队协作，跨机器）

```json
{
  "lockfileVersion": 1,
  "generatedBy": "heron/1.0.0",
  "targets": [
    { "kind": "claude", "scope": "project" },
    { "kind": "agents", "scope": "project" }
  ],
  "skills": {
    "find-skills": {
      "source": "https://github.com/anthropics/skills.git",
      "ref": "main",
      "subpath": "skills/find-skills",
      "commit": "5f2d1c0e9a7b3f8d2c4e6a1b9d7f5e3c1a2b4d6e",
      "version": "1.4.0",
      "contentHash": "blake3:9f2c…",
      "projection": "auto"
    },
    "huashu-design": {
      "source": "local",
      "path": "./skills/huashu-design",
      "contentHash": "blake3:4bad…"
    }
  }
}
```

> 锁文件只记录版本/提交与内容指纹，不记录绝对路径；导入时逐条校验哈希，不一致则报告漂移项。

---

## 8. 功能设计

### 8.1 功能地图

```text
Heron
├── F1 发现 (Discover)
│   ├── 扫描已知位置与项目目录
│   ├── 解析 SKILL.md（容错）
│   ├── 识别旧式命令文件 (.claude/commands/*.md)
│   └── 产出"已纳管 / 未纳管 / 可疑"三类清单
├── F2 收拢 (Adopt)
│   ├── 单条 / 批量收拢
│   ├── 同内容自动去重（多份拷贝 → 一份实体）
│   ├── 备份 + 原位置生成投影
│   ├── 兼容 skills-lock.json 回填来源
│   └── 旧式命令文件接管（legacy-command）
├── F3 库管理 (Library)
│   ├── 列表 / 搜索 / 标签 / 收藏
│   ├── 详情：来源、版本历史、本地改动、落点
│   ├── 编辑（打开 Store 内副本）
│   └── 软删除与恢复（trash）
├── F4 分发 (Project)
│   ├── 开关式启用（客户端 × 项目）
│   ├── 投影方式切换（copy / symlink / junction / pointer）
│   └── 漂移检测与修复
├── F5 来源 (Sources)
│   ├── 内置默认市场 + 自定义 Git / 本地来源
│   ├── GitHub / GitLab / Gitee / SSH / 私有地址解析
│   ├── 浏览与安装（含单个 Skill 子目录深链）
│   └── 缓存与离线
├── F6 升级 (Upgrade)
│   ├── 检查更新（outdated）
│   ├── 差异预览（本地改动 vs 上游改动）
│   ├── 三方合并升级
│   ├── 回滚到任意版本
│   └── 自动检查策略（pin）
├── F7 体检 (Doctor)
│   ├── 重复 / 冲突 / 坏链 / 漂移
│   ├── 不可见（未扫描 / 被压制 / 需重启 / 客户端原生扫描状态）
│   ├── 不合规 frontmatter / 病态嵌套 / 丢失来源
│   └── 一键修复（可预览）
├── F8 团队 (Lock)
│   ├── 导出 heron.lock
│   ├── 导入 / 复现
│   └── 漂移报告
└── F9 系统 (System)
    ├── 设置 / 路径覆盖 / 语言
    ├── 日志与诊断包
    └── 自动更新 / 关于
```

### 8.2 关键流程

#### 流程 A：收拢（Adopt）

```text
用户选中 N 条"未纳管"条目
  │
  ├─▶ 1. 预检
  │       · frontmatter 是否合规？
  │       · 内容哈希是否与已有 slug 相同？同哈希 → 标记"重复，建议合并"
  │
  ├─▶ 2. 生成计划（Dry-run）
  │       · 复制至 store/<slug>（或复用已有同哈希实体）
  │       · git init + 首次提交 + 创建 upstream 分支
  │       · 原目录移入 backups/<ts>/…，原位置替换为投影（默认 auto）
  │       · 命中 skills-lock.json 的来源 → 写入 source_binding
  │
  ├─▶ 3. 用户确认（GUI 展示完整影响面；CLI 需 --yes）
  │
  └─▶ 4. 执行（逐步事务）
         每步写 event 表；失败按逆序回滚已完成步骤
```

**不可破坏原则**：原目录**永远**先备份后改动；备份失败则整个收拢终止。

#### 流程 B：从远程安装

```text
输入（例）：
  · https://github.com/v5note/wechat-mp
  · github:anthropics/skills#main:skills/wechat-mp
  · /local/path/to/skill
  · 在来源浏览页勾选
  │
  ├─▶ 解析 URL → (url, ref, subpath)，并展示解析结果供确认
  ├─▶ 查 sources-cache（命中则复用裸仓）
  ├─▶ git fetch --depth=1 <ref> → 读取 tree
  ├─▶ 定位 Skill 根（含 SKILL.md 的目录；若给了文件路径则取其父目录）
  ├─▶ 校验 frontmatter；生成 slug（name 规范化；冲突则加后缀）
  ├─▶ 写入 Store：初始提交 → branch upstream 指向远端 commit
  ├─▶ 记录 source_binding（含 pinned commit 与内容哈希）
  └─▶ 询问 / 应用默认投影（默认不自动投影，避免上下文膨胀）
```

#### 流程 C：升级（含本地改动）

```text
检测阶段
  git fetch source → 比较远端 HEAD 与本地基线
  分类：up_to_date / has_update / pinned / local_modified / diverged / upstream_gone
  │
升级阶段（单个 Skill）
  1. base   = heron.skill.json 记录的基线 commit
  2. ours   = branch local 的 HEAD（工作区存在未提交改动则先暂存为临时提交）
  3. theirs = 新拉取的 commit
  4. 三方合并：无冲突则得到合并树
        · 有冲突 → 产出冲突文件清单，GUI 展示三栏，用户逐块选择
  5. 校验合并结果（frontmatter 有效、无二进制冲突）
  6. 更新 upstream 分支、写入版本 tag 与 version 行、更新基线
  7. 刷新所有相关投影（copy 模式重写；link 模式不动）
  8. 失败 ⇒ 回退到操作前状态，投影不变
```

#### 流程 D：体检与修复

| 检查项 | 判定 | 修复动作（需用户确认） |
|--------|------|------------------------|
| 重复实体 | 仓库内 ≥2 个 slug 内容哈希相同 | 合并为一个，其余删除或加别名 |
| 同名冲突 | 同一目标下两个落点同名 | 提示保留项或改名 |
| 坏链 | 投影指向不存在的路径 | 修复重指或移除 |
| 漂移 | 投影被外部改成实体目录 / 被改动内容 | 重新投影（先备份被改内容） |
| 不可见 | 落点不在该客户端扫描路径内 | 建议改到正确目标 |
| 被压制 | 同名条目在该客户端优先级更高 | 报 shadowed-by，不自动删除 |
| 需重启 | 落点出现在会话启动之后 | 提示重启会话 |
| frontmatter 不合规 | name 缺失/非法；description 缺失或超 1024 字符 | 提供就地修正建议（不擅自改上游文件） |
| 病态嵌套 | `skills/.claude/skills/…` 或 `skills/.agents/skills/…` | **只报告 + 给建议，绝不自动修改**（D8）；用户可选择：忽略 / 手动处理 / 导出诊断包 |
| 丢失来源 | 已纳管但无 source_binding | 按内容哈希在已知来源中反查 |

### 8.3 旧式命令文件的接管（对应 D9）

```text
发现阶段
  ~/.claude/commands/review.md          → legacy-command，slug 候选 "review"
  <proj>/.claude/commands/git/commit.md → legacy-command，slug 候选 "git:commit"
  │
收拢阶段
  1. 生成 store/<slug>/SKILL.md（内容 = 原命令文件）
  2. heron.skill.json 记录 origin=legacy-command 与原绝对路径
  3. 原文件移入 backups/<ts>/…，原位置可选：保留（默认）或替换为指针文件
  │
使用阶段
  · 可投影到任意目标（含 .claude/commands/<slug>.md 的"回投影"形态）
  · 体检标记 legacy-command 并提示迁移
```

**默认动作**：收拢时**保留**原命令文件（只做备份与登记，不删除、不改写），因为旧客户端仍在读取它；是否改写为指针由用户在收拢预览中勾选。

---

## 9. 来源与远程仓库规范

### 9.1 来源类型

| 类型 | 示例 | 说明 |
|------|------|------|
| `builtin` | Heron 内置默认市场 | 只读，可禁用；离线有缓存 |
| `git` | `https://github.com/owner/repo.git` | 通用 Git；支持 ref / subpath |
| `github` | `owner/repo` 简写 | 解析为 `https://github.com/owner/repo.git` |
| `local` | `/Users/me/skills` | 本地目录，可监听自动同步 |
| `marketplace-compat` | 含 `.claude-plugin/marketplace.json` 的仓库 | 只读消费其 plugin 列表中的 Skill |

### 9.2 URL 解析规则（含歧义处理）

| 输入形态 | 解析结果 | 规则 |
|----------|----------|------|
| `owner/repo` | git + 默认分支 | 仅允许两段，且不含冒号或空格 |
| `https://github.com/o/r` | git + 默认分支 | |
| `https://github.com/o/r/tree/<ref>/<path>` | git + ref + subpath | `/tree/` 之后第一段为 ref |
| `https://github.com/o/r/blob/<ref>/<path>/SKILL.md` | 同上去掉文件名 | |
| `git@host:group/repo.git` | git + SSH | |
| `https://gitlab.com/组/子组/仓库` | **歧义** | GitLab 支持深层子组，无法仅凭 URL 判断仓库边界 → **试探法**：先探测 `…/repo.git/info/refs`，失败则逐段收缩；GUI 展示"仓库识别为 X，子目录为 Y"供修改 |
| `gitlab:group/sub/repo` 显式前缀 | git | **推荐写法**，消除歧义 |
| `.zip` / `.tar.gz` | archive | Phase 3；Phase 1 提示解压后拖入 |
| 本地路径 | local | 存在且含 SKILL.md 则直接可用 |

**设计原则**：来源解析**永不静默猜测**——有歧义时展示解析结果由用户确认（GUI），或要求显式 `--vcs gitlab`（CLI）。

### 9.3 Catalog 优先级

```text
1. .claude-plugin/marketplace.json   （兼容 Claude marketplace，多插件清单）
2. heron.source.toml                 （Heron 原生）
3. .claude-plugin/plugin.json        （兼容 Claude 单插件清单，`skills[]` 声明即源）
4. 直接扫描仓库中的 SKILL.md         （兜底，支持 max_depth 与排除）
```

* **兼容层映射规则**：
  * `plugins[].name` → 包名；
  * `plugins[].source` 为相对路径 → 该目录下 `skills/*` 即为其 Skill；
  * `plugins[].source` 为 `github` / `url` / `git-subdir`（含 ref / sha）→ 记为"外部包"，Phase 1 展示并标记为需二次拉取；
  * plugin 的 `version` 若存在，作为 Skill 版本来源；
  * `plugin.json` 的 `skills[]` 相对路径直接作为 Skill 目录，指向不存在 `SKILL.md` 的条目跳过。
* **兜底扫描排除客户端投影副本**：顶层点目录（`.trae/`、`.claude/`、`.doubao/` …）下的 `SKILL.md`
  视为作者预先投影的副本，仅在仓库中没有其它候选时才采用——投影是 Heron 的职责，不该被当成来源。
* **Heron 原生格式**（简洁、面向 Skill）：

```toml
schema = 1
name = "团队内部市场"

[[skill]]
slug = "code-review"          # 可选；缺省取 SKILL.md 的 name
path = "skills/code-review"   # 相对仓库根
version = "1.2.0"             # 可选
tags = ["review"]
homepage = "https://…"
```

* **兜底扫描**：`max_depth = 6`，跳过 `.git` / `node_modules` / 测试夹具目录；遇到"含 SKILL.md 的目录"即视为边界，不再向内递归。

### 9.4 安全与信任

* 首次从**非 trusted** 来源安装时，GUI 必须展示内容摘要（是否含 `scripts/`、是否引用网络命令）。
* Heron **绝不自动执行** Skill 中的任何脚本。
* 私有仓库依赖系统 git 凭据（SSH agent / credential helper），Heron 不存储密码。

### 9.5 内置市场适配层（实测结论，评审决议 D1）

五个市场的能力差异极大，**不能用同一套实现假装统一**。Heron 引入 `MarketAdapter` 抽象（见 §9.5.1），按各家真实能力分成三档接入：

| 能力档 | 含义 | 默认市场 |
|--------|------|----------|
| **A. 可索引 + 可一键安装** | 有机器可读接口且能拿到内容或可解析到 Git 源，可在 Heron 内搜索、安装、检查更新 | skills.sh、clawhub.ai |
| **B. 可引导** | 有站点可浏览，但无稳定公开 API 或有反爬，Heron 只能"打开到指定搜索页"并引导用户复制地址回投 | qoder.com、skillhub.cn、skillsmp.com |
| **C. 已废弃/不可达** | 站点不存在或无可用接口 | — |

**2026-11-19 实测记录（作为适配层基线，后续由 probe 任务复核）**：

| 市场 | 站点可达 | 机器接口 | 目录↔Git 源 | 结论档位 |
|------|----------|----------|-------------|----------|
| **skills.sh** | ✅ | ⚠️ 无公开 JSON API；目录结构可从 `sitemap-*.xml` 推导（`/<owner>/<repo>/<skill>`），详情页含 `github.com/<owner>/<repo>` 链接 | ✅ 天然指向 GitHub 仓库（即 `npx skills` 生态） | **A 档**：适配器以"Git 仓库索引"方式接入，安装即 `heron add https://github.com/<owner>/<repo> --subpath skills/<skill>`；解析失败时降级为 B 档引导 |
| **clawhub.ai** | ✅ | ✅ 有 REST API：`GET /api/v1/packages`（游标分页）、`/api/v1/packages/<name>`、`/api/v1/packages/<name>/versions`、`/api/v1/skills`、`/api/v1/search?q=`（返回 `install.reference` 与 `links.source`） | ⚠️ 部分条目 `sourceUrl` 为空（托管在 platform 而非 Git） | **A 档（有条件）**：有 Git 源时走 Git 安装；无 Git 源时 Phase 1 只展示来源并提示"暂不支持直装"，Phase 2 再补内容下载通道 |
| **qoder.com** | ✅ 市场页可达 | ❌ 未发现公开 API，页面为客户端渲染 | ❌ | **B 档（引导式）** |
| **skillhub.cn** | ✅ 首页可达 | ❌ 页面为纯 JS 渲染（`robots.txt` 全站 Disallow），仅有 sitemap | ❌ | **B 档（引导式）** |
| **skillsmp.com** | ⚠️ 返回挑战页（反爬） | ❌ | ❌ | **B 档（引导式），默认关闭** |

```text
                     ┌──────────────── MarketAdapter ────────────────┐
   GUI 来源页  ──────▶ │ search() · get() · resolveInstall() · probe() │
   heron search     ──▶ └───────┬───────────────────────┬───────────────┘
                                │                       │
                     GitIndexAdapter            DeepLinkAdapter
                     (skills.sh / clawhub 有源)  (qoder / skillhub / skillsmp)
                                │                       │
                     返回 (repo, ref, subpath)   返回 (openUrl, hint)
                                │                       │
                      走 §8.2 流程 B 安装        在系统浏览器打开，用户复制地址回投
```

#### 9.5.1 `MarketAdapter` 接口（领域层定义，Phase 1 三个实现）

```text
trait MarketAdapter {
    fn id(&self) -> MarketId;                     // skills_sh / clawhub / qoder / skillhub_cn / skillsmp
    fn capability(&self) -> Capability;           // A / B
    fn search(&self, q: &Query) -> Result<Page<MarketItem>>;
    fn get(&self, key: &MarketKey) -> Result<MarketItem>;
    fn resolve_install(&self, item: &MarketItem) -> Result<InstallPlan>;  // A 档：GitPlan；B 档：DeepLink
    fn probe(&self) -> Result<Health>;            // 供"来源可用性"与自动降级
}
```

* 所有适配器**必须可离线降级**：接口失败时返回上次缓存（`sources-cache/market/<id>/`）并在界面标注"数据可能过期"。
* 适配器**不得**在后台静默下载或执行任何内容；`resolve_install` 仅产出计划，实际安装仍走 §8.2 流程 B 的用户确认。
* 每个市场的 `verified_at` / `last_probe_ok` 写入 `source` 表，体检页可一键重探。

#### 9.5.2 引导式（B 档）的用户体验

引导式不等于不可用，它是"可发现但需人工承接"：

1. Heron 生成带关键词的深链（如 `https://qoder.com/zh/marketplace?q=<关键词>`），一键在系统浏览器打开；
2. 用户找到 Skill 后复制其 GitHub 地址；
3. Heron 在界面提供"粘贴地址回投"入口 —— 直接进入 §8.2 流程 B，**无需理解仓库结构**。

> 这条路径同时服务"用户拿到的任意 GitHub 地址"，因此 B 档市场的边际实现成本很低。

---

## 10. 核心机制设计

### 10.1 存储与投影模型

```text
        ┌────────────┐  install / upgrade   ┌──────────────────┐  project   ┌──────────────┐
        │  Source    │ ───────────────────▶ │      Store       │ ─────────▶ │  Projection  │
        │  (Git/本地)│                      │ (受管，真相源)   │            │  (目标落点)  │
        └────────────┘                      └──────────────────┘            └──────────────┘
                                                    │                            │
                                             版本历史 / 本地改动            可见性 / 漂移
```

四种投影方式对比：

| 方式 | 平台 | 优点 | 缺点 | 适用 |
|------|------|------|------|------|
| `copy` | 全平台 | 兼容任何扫描器 | 需同步；目标被改后回写需显式操作 | 兜底；不识别链接的工具 |
| `symlink` | macOS / Linux / Windows（开发者模式） | 零同步 | 部分扫描器对目录项判定异常 | macOS 默认 |
| `junction` | Windows | 零权限、零同步 | **删除语义危险**（见 §10.2） | Windows 默认 |
| `pointer` | 全平台 | 内容极简，可绕开链接判定问题 | 目录名固定，依赖客户端能读指针文件 | 特殊扫描器兜底 |

**copy 模式的一致性保证**：投影记录 `content_hash`；每次 project / upgrade / 启动对账时重算目标哈希：

* 目标哈希 == 记录哈希 → `ok`；
* 目标哈希 != 记录哈希 → `modified`（用户在目标处改了东西）→ **不覆盖**，提示"迁回 Store / 放弃本地改动"；
* 目标不存在 → `broken`。

### 10.2 Windows 链接与 reparse point（头号实现陷阱）

| 需求 | 做法 |
|------|------|
| 判定类型 | 用 `FILE_ATTRIBUTE_REPARSE_POINT` + reparse tag 区分：`IO_REPARSE_TAG_SYMLINK (0xA000000C)` 与 `IO_REPARSE_TAG_MOUNT_POINT (0xA0000003，junction)`。**不能**只靠 `symlink_metadata().is_symlink()`，它把 junction 也算作符号链接 |
| 删除投影 | **先**解除链接本身（删除 reparse point），**不可**直接调用会递归删除目录的 API，否则可能连带删除目标内容。救援手段：`fsutil reparsepoint delete <path>`（需管理员，仅用于异常恢复） |
| 创建 junction | 标准库无直接 API：通过 `FSCTL_SET_REPARSE_POINT` 自建，或使用成熟封装 crate（实现文档定版）。创建前断言目标存在且为目录 |
| 创建 symlink | `std::os::windows::fs::symlink_dir`；失败（`ERROR_PRIVILEGE_NOT_HELD`）→ 自动降级为 junction 并记录日志 |
| 长路径 | 所有 FS 调用统一经 `heron-platform::fs` 包装，内部使用扩展路径前缀；界面展示时转回常规路径 |
| 网络盘 / OneDrive | 检测到 Store 或目标位于网络盘 / OneDrive 目录时给出警告（链接可能被占位符化） |

### 10.3 内容指纹与"改动检测"

* **规范化**：以 `SKILL.md` 所在目录为根递归收集文件；忽略 `.git`、`.DS_Store`、`Thumbs.db`、`.heron`；路径统一为正斜杠；**换行符统一为 LF**（Windows 检出不应被判为修改）；文件权限位不参与哈希；符号链接按"链接目标字符串"计入。
* **指纹算法**：对 `(相对路径, 文件字节)` 排序后依次哈希，输出 `blake3:<hex>`。
* **用途**：同内容去重、升级检测、copy 模式漂移检测、锁文件校验。

### 10.4 版本与升级策略

**版本来源优先级**：

1. 来源 catalog 给出的 `version`；
2. 仓库 tag 且语义化（`v1.2.3` 或 `wechat-mp-v1.2.3`）；
3. `SKILL.md` frontmatter 的 `metadata.version`；
4. 兜底：`0.0.0+<short sha>`（**永不静默**，体检中提示补版本号）。

**pin 策略**：

| pin | 行为 | 适用 |
|-----|------|------|
| `exact` | 锁死当前 commit，永不自动更新，`outdated` 也不提示 | 生产项目、需要极致稳定的 Skill |
| `semver`（默认） | 允许同 major 内升级；跨 major 需显式确认 | 大多数场景 |
| `none` | 跟随来源 ref 的 HEAD | 个人开发中的 Skill |

**三方合并的基线**：`heron.skill.json` 记录 `baseCommit`（上次成功合并时的上游 commit）与 `baseTreeHash`。每次升级成功后更新基线；本地改动每次提交到 `local` 分支，历史完整可回溯。

**回滚**：`rollback --to <version>` 等价于把 `local` 分支重置到目标版本的 tag（重置前自动打 `rollback-backup` 标签）并刷新投影；回滚本身也可再次回滚。

**无 Git 环境（评审决议 D7）**：Heron **引导用户安装 Git**，不做静默降级。

| 平台 | 引导方式 |
|------|----------|
| macOS | 检测 `xcode-select -p` 是否可用；缺失时给出 `xcode-select --install` 与 Homebrew 两种方式 |
| Windows | 提供 Git for Windows 下载页与内置的 winget 命令（`winget install --id Git.Git`），并支持自动打开安装页 |
| 通用 | 允许配置 `git.binary` 指向自定义路径；安装完成后一键重试 |

**降级预案（默认不启用）**：仅在用户**显式**选择"暂不使用 Git"时生效——每个版本落成 `store/<slug>/.versions/<hash>/` 快照，可浏览与导出，但**禁止**安装、升级、回滚操作，界面常驻横幅提示"当前为只读预览模式"。

### 10.5 GUI 外壳的边界条件

* **WebView2（Windows）**：Win11 默认内置；Win10 需 1803+ 且安装 Evergreen Runtime。打包可选 online（体积小）与 offline（内置引导，体积大），默认 online 并在失败时给出安装引导。
* **WKWebView（macOS）**：系统内置，无额外依赖；最低目标版本 **macOS 12.0+**。
* **签名与公证**：macOS 需 Developer ID 签名 + 公证；Windows 建议代码签名，否则出现 SmartScreen 提示。列入实现文档的发布清单。

### 10.6 性能与规模

| 场景 | 目标 | 手段 |
|------|------|------|
| 首次全盘扫描 | 5 万目录 < 10 s（SSD） | rayon 并行；只读 `SKILL.md` 头部（≤ 64 KB）解析元数据；路径与 mtime 级缓存 |
| 增量刷新 | < 500 ms | `notify` 监听 + 索引表比对 path / mtime / size |
| 内容哈希 | 仅对变化项重算 | 以缓存的哈希 + 大小/时间戳做快捷判定 |
| 升级检查 | 10 个来源 < 15 s | 并发 fetch；浅取；结果按 `auto_upgrade_check` 周期缓存 |
| 大仓库 | 只取需要子目录 | `git sparse-checkout` + partial clone（`--filter=blob:none`） |

### 10.7 并发、锁与 IPC

* **单实例**：`tauri-plugin-single-instance` 保证 GUI 唯一；CLI 通过 `ipc.json` 发现 daemon。
* **文件锁**：结合 SQLite WAL 与 Store 级锁；跨进程写操作（install / upgrade / adopt）串行化。
* **事务性**：所有"多文件改动"包装为 Operation，每步登记补偿动作（undo），异常时逆序执行；`event.undoable=1` 的条目支持 GUI"撤销上次操作"。

### 10.8 错误分类与可恢复性

| 错误码 | 含义 | 用户可见要点 | 恢复 |
|--------|------|--------------|------|
| `E_GIT_MISSING` | 未检测到 git | 引导安装或切换降级模式 | 安装后重试 |
| `E_NETWORK` | 网络 / 代理 / 认证失败 | 展示 git stderr 摘要与建议 | 重试 / 离线模式 |
| `E_SOURCE_AMBIGUOUS` | URL 无法唯一解析 | 展示候选并请用户选择 | 用户输入显式形式 |
| `E_FRONTMATTER` | Skill 不合规 | 指出具体字段 | 修正后再装 |
| `E_SLUG_CONFLICT` | slug 已存在且内容不同 | 选择"覆盖 / 改名 / 并存" | 用户决策 |
| `E_LINK_PRIVILEGE` | 无创建链接权限 | 建议切换为 copy 投影 | 一键降级 |
| `E_DRIFTED` | 目标被外部改动 | 展示差异 | 迁回 / 覆盖（先备份） |
| `E_MERGE_CONFLICT` | 合并冲突 | 三栏合并界面 | 手工解决 |
| `E_UPSTREAM_GONE` | 上游删除或分支改名 | 提示并保留本地副本 | fork 为本地 |

### 10.9 容错与"脏数据"处理

| 场景 | 处理策略 |
|------|----------|
| frontmatter 非标准 YAML（多行 description、未加引号的冒号） | 宽松解析：先标准解析，失败回退到"仅提取顶层标量"的容错解析；GUI 标记"解析降级" |
| 大小写 / 编码异常（BOM、CRLF） | 统一归一化后再解析，原始文件不改写 |
| 目录名即 Skill 名但 frontmatter name 不一致 | slug 取 frontmatter name（缺失才取目录名）；落点名默认取 slug；不一致在体检中提示 |
| 同名不同内容 | 安装时加后缀（如 `code-review-2`）并提示；不静默覆盖 |
| Store 内容被外部直接修改 | 下次启动检测到不一致 → 标记 `local_modified` 并提示提交为本地改动（不是错误） |
| 索引损坏 | `heron doctor --rebuild-index`：从 Store 与文件系统重建（索引永远是可推导的） |

### 10.10 国际化（评审决议 D4）

第一版同时支持**简体中文（zh-CN）与英语（en-US）**，采取"内核产出稳定标识 + 前端渲染文案"的分层方案：

| 层 | 做法 |
|----|------|
| 内核（Rust） | **不拼接面向用户的自然语言**；错误与提示以结构化形式返回：`{ code, params: {…}, severity }` |
| 前端（Vue） | 以 `vue-i18n` 渲染 `code → 文案模板`，参数做占位替换；语言包按命名空间拆分（`common / skill / source / upgrade / doctor`） |
| CLI | 读取 `LANG` / 配置 `general.language` 选择语言包；`--json` 输出永远只含 `code + params`，不受语言影响 |
| 文档/市场名 | 市场名称与 Skill 描述保持原文（不翻译），仅站点 UI 文案做本地化 |
| 日期/数字 | 统一按用户区域格式化，存储层一律 UTC + ISO 8601 |

**约束**：

* 文案不得出现在 Rust 源码的 UI 路径中（由 CI 静态检查 `t!(…literal…)` 形式兜底）；
* 中文与英文语言包的 key 集合必须完全一致，CI 校验缺失/多余 key；
* 新增文案的 PR 必须同时提交两种语言，否则 CI 失败。

---

### 10.11 客户端原生扫描（评审决议 D6）

> **口径说明**：本节的定义是"读取客户端自身的配置（判断它实际加载了什么、同名谁胜出），并在**用户确认后**把某目录写成客户端**原生扫描**的目录"——两者都以"不改坏客户端"为底线。

#### 10.11.1 只读识别（永远启用、风险为零）

| 对象 | 只读来源 | 用途 |
|------|----------|------|
| Claude Code | `~/.claude/settings.json` / `<proj>/.claude/settings.json` 中的 `enabledPlugins` 等；`~/.claude/plugins/known_marketplaces.json`、`installed_plugins.json` | 判定某 Skill 是否 `owned-by-plugin`、是否被插件压制 |
| Codex / 通用 agent | `~/.codex/`、`~/.agents/skills/` 的目录事实 + `skills-lock.json` | 判定来源与是否由 `npx skills` 管理 |
| 各客户端 | `<proj>/.claude|.agents|.codex|.cursor|.opencode` 目录事实 | 计算 `verify` 的 `visible / shadowed / not_scanned` |

只读识别**不写入任何客户端文件**，可在任意时刻执行；结果落 `visibility` 表。

#### 10.11.2 原生扫描注入（需用户显式确认、可一键撤销）

**目的**：客户端升级后扫描路径变化时，仍有一条"客户端自己认"的兜底通道，而不是依赖我们维护的路径表。

| 客户端 | 是否支持注入 | 注入方式（调研确定，实现前需真机验证） | 风险与约束 |
|--------|--------------|------------------------------------------|------------|
| Claude Agent SDK / `--add-dir` 场景 | ✅ 有官方机制 | 仅用于**用户自己的启动脚本**，Heron 不代改 | 低 |
| Claude Code | ⚠️ 无文档化的"额外 skill 目录"配置 | Heron **不自行发明**配置项；若未来官方提供，则通过 `clients/*.toml` 的 `native_scan` 能力表热更启用 | 高 |
| Codex / 通用 | ⚠️ 同上 | 同 Claude 处理方式：能力表驱动，不硬编码私有配置 | 高 |
| 所有客户端 | 兜底通道 | 投影到各客户端**已文档化的扫描目录**（`.claude/skills` 等），这正是 §10.1 的 `project` | 中 |

**设计原则**：

1. **能力表驱动**：注入能力写在 `clients/*.toml` 的 `native_scan` 字段（`supported / method / config_path / evidence`），随版本热更，不在代码里写死任何客户端的私有配置格式。
2. **默认关闭**：`config.toml` 的 `native_scan.enabled = false`；开启必须逐客户端确认，并在界面展示"将写入哪个文件、写入什么内容"的**完整 diff**。
3. **可逆**：每次注入前备份原文件到 `backups/native-scan/<ts>/`，并记录到 `event`；`heron verify --revert-native` 可一键还原。
4. **幂等与冲突检测**：写入前检测用户的改动（文件哈希与上次记录不一致则拒绝覆盖并提示）。
5. **不猜**：无法确证格式的配置，Heron **宁可不写**，改为提示"投影到已文档化目录"。

> 结论：**D6 = 只读识别（全量）+ 注入（能力表驱动 + 用户确认 + 默认关闭）**。这既满足"接管"的目标，又避免把 Heron 绑死在某个客户端的私有配置格式上。

### 10.12 无 Git 时的引导（对应 D7）

见 §10.4「无 Git 环境」。补充：

* 启动时 preflight 在**并行**执行，不阻塞界面首屏；缺失时横幅可关闭但不消失（去设置里可静默）。
* CLI 场景下 `heron scan` / `list` / `doctor` **不需要 Git**，只有 `install / upgrade / rollback / project` 需要；因此"没装 git"不等于"Heron 不可用"。
* 错误码 `E_GIT_MISSING` 附带 `install_hint` 与 `retryable: true`。

### 10.13 病态嵌套的处理边界（对应 D8）

| 类别 | 示例 | Heron 行为 |
|------|------|------------|
| 目录嵌套 | `skills/.claude/skills/…`、`skills/.agents/skills/…` | **只报告**，列出"疑似应为外层目录的 Skill"与建议去向；不移动、不删除 |
| 自我引用 | `skills/<x>/SKILL.md` 内部又指向 `references/skills/…` | 只报告 |
| 扫描环 | 符号链接成环 | 扫描器直接跳过（安全行为），报告中标注"因成环而跳过" |
| 用户处置 | — | 提供三项动作：`忽略本次`（写入 ignore 列表）、`打开所在目录`、`导出诊断包` |

> 反复强调：**Heron 不替用户做破坏性整理**。所有修复动作都要求用户逐条确认，且必须先备份。

### 10.14 旧式命令文件接管（对应 D9）

Claude Code 的 `.claude/commands/*.md` 是 Skill 的前身：单文件、frontmatter 相似、同样按目录名/文件名为触发名。

| 维度 | 处理方式 |
|------|----------|
| 发现 | 扫描时把 `.claude/commands/**/*.md` 识别为 `legacy-command`；以文件名（去扩展名，子目录用 `dir:name`）作为候选 slug |
| 收拢 | 导入为 Skill：生成目录 `store/<slug>/SKILL.md`，内容为原命令文件；在 `heron.skill.json` 记 `origin: legacy-command` 与原路径 |
| 投影 | 可作为普通 Skill 投影到任意目标；同时支持**回投影**到 `.claude/commands/<slug>.md`（保持旧客户端可用） |
| 兼容差异 | `name` 字段可缺失（取文件名）；`allowed-tools` 语义与 Skill 一致；不支持的字段原样保留 |
| 体检 | 标记 `legacy-command`，提示"建议逐步迁移为 Skill 目录形态" |
| 冲突 | 同名 Skill 与 command 同时存在时，二者**并存**（客户端语义不同），在界面合并展示并标注来源 |

### 10.15 Store 与映射点（对应 D10）

```text
~/.heron/skills/<slug>/             唯一真相源（含 .git），不是投影目标
        │
        ├──▶ ~/.agents/skills/<slug>          映射点（跨客户端共享，与 npx skills 共享所有权）
        ├──▶ ~/.claude/skills/<slug> …        客户端自有目录（按需）
        └──▶ <proj>/.claude/skills/<slug>     项目级目标（需显式启用）
```

| 位置 | 默认状态 | 说明 |
|------|----------|------|
| `~/.heron/skills` | **Store，始终启用** | 由 Heron 完全拥有，不受第三方工具变更影响；不出现在目标清单里，不参与扫描 |
| `~/.agents/skills` | 关闭（映射点） | 启用时 **必须** 先检测 `skills-lock.json` / `npx skills` 的存在，并提示共享风险；Heron 只增删自己创建的投影，不动其它条目 |
| `~/.claude/skills` 等 | 关闭 | 仅在 `heron project <slug> --on <target>` 明确点名时投影 |
| `<proj>/.claude/skills` 等 | 关闭（需用户对具体项目开启） | 项目级目标 |

**没有"默认目标"**：`heron project` / `heron unproject` 不带 `--on` 时报错并列出当前已启用的目标 id，避免把 Skill 悄悄写进用户没选过的客户端目录。

**与 `npx skills` 的共存规则（明确写入产品文档）**：

1. Heron 在 `.agents/skills` 里**只碰自己创建的**条目（以 `projection` 表为准）；
2. 收拢 `npx skills` 安装的条目时，保留其 lock 信息到 `source_binding`，并在用户确认后把原实体迁入 Store、原位改为投影；
3. 若 `npx skills` 后续覆盖了 Heron 的投影，体检会报 `drift` 而非静默覆盖回去。

---

## 11. 命令面（CLI）

> CLI 与 GUI 能力同源。动词统一为 `list / get / add / rm / set` 风格，便于 Phase 3 独立演进。

```text
heron                                   # 启动 GUI（等价 heron gui）
heron gui [--cwd <path>]

# 发现与收拢
heron scan [--roots <p>...] [--json]
heron detected [--json]                 # 未纳管清单
heron adopt <path|slug>... [--all] [--dry-run] [--yes]
heron list [--client <c>] [--project <p>] [--unmanaged] [--json]
heron get <slug> [--json]
heron search <keyword> [--remote] [--json]

# 分发
heron project <slug> --on <target>[,<target>...] [--mode copy|symlink|junction|pointer]
heron project <slug> --everywhere
heron unproject <slug> [--on <target>] [--force]
heron targets list | add <name> <path> | rm <name> [--default]

# 来源
heron source list | add <url> | rm <id> | sync [--all]
heron source browse <id> [--json]
heron add <url-or-source-ref> [--ref <r>] [--subpath <p>] [--on <target>...]

# 升级
heron outdated [--json]
heron upgrade [<slug>...] [--all] [--dry-run] [--to <ver>] [--strategy merge|theirs|ours]
heron rollback <slug> --to <version>
heron diff <slug> [--against <version|commit>]

# 体检
heron doctor [--fix] [--json]
heron verify [<slug>] [--json]
heron native-scan status|enable <client>|disable <client>|revert [--client <c>]   # 客户端原生扫描（D6）
heron commands list|adopt <name>                     # 旧式命令文件接管（D9）

# 编辑与团队
heron edit <slug> [--editor]
heron fork <slug> [--as <new-slug>]
heron new <slug> [--template basic|scripted]
heron lock export [--out heron.lock]
heron lock import <heron.lock> [--dry-run]
heron sync [--cwd <project>]            # 按 heron.lock 复现

# 系统
heron config get|set|list
heron trash list | restore <id> | empty
heron log [--follow]
heron cache clean
```

**交互原则**：

* 任何会改动目标目录的写操作默认打印计划，`--yes` 跳过；
* 破坏性动作（`unproject --force`、`trash empty`、`--strategy theirs`）必须先展示受影响路径清单；
* 全部命令支持 `--json`；退出码：`0` 成功、`1` 一般错误、`2` 用法错误、`3` 需要用户决策、`4` 部分成功。

---

## 12. 界面设计

### 12.1 总体布局

```text
┌──────────────────────────────────────────────────────────────────────────┐
│  Heron 苍鹭                  🔍 搜索 45 个 Skill       [检查更新 3]    ⚙  │
├──────────┬───────────────────────────────────────────────────────────────┤
│ 概览      │  Skill：huashu-design             v3.0.0   [有更新 → v3.2.0]  │
│ 我的 Skill│  ─────────────────────────────────────────────────────────    │
│ 发现      │  来源  github.com/alchaincyf/huashu-design · main · 3 天前   │
│ 来源      │  本地改动  无                               [查看历史]        │
│ 项目      │  ─────────────────────────────────────────────────────────    │
│ 更新      │  启用位置                                                     │
│ 体检      │  ┌──────────────┬─────────────┬──────────┬───────────────┐    │
│ 设置      │  │ 客户端        │ 项目         │ 方式      │ 状态           │    │
│          │  ├──────────────┼─────────────┼──────────┼───────────────┤    │
│          │  │ Claude Code  │ 全局         │ symlink  │ ● 生效中       │    │
│          │  │ Claude Code  │ ~/proj/a    │ copy     │ ● 生效中       │    │
│          │  │ Codex(.agents)│ 全局        │ junction │ ◐ 需重启会话   │    │
│          │  └──────────────┴─────────────┴──────────┴───────────────┘    │
│          │  [ + 启用新位置 ]                                             │
├──────────┴───────────────────────────────────────────────────────────────┤
│ 后台任务：正在检查 10 个来源的更新…  ████████░░ 80%                       │
└──────────────────────────────────────────────────────────────────────────┘
```

### 12.2 关键页面

| 页面 | 内容 | 关键交互 |
|------|------|----------|
| **概览** | 统计卡（Skill 数、未纳管数、可升级数、体检问题数）、最近操作、健康度 | 一键跳转处置 |
| **我的 Skill** | 表格 / 卡片双视图；列：名称、版本、来源、启用位置数、状态标记 | 搜索过滤、批量启用/停用 |
| **发现** | 未纳管清单，按来源目录分组，标出重复组 | 勾选批量收拢；预览收拢计划 |
| **来源** | 已注册来源列表（含五个内置市场，标注"可一键安装 / 引导式"与可用性） | 添加来源向导（粘贴 URL → 解析确认 → 预览目录） |
| **市场浏览** | 选中市场后的检索与卡片列表：名称、作者、下载量、是否有 Git 源 | A 档：直接安装；B 档：`在浏览器中打开` + `粘贴地址回投`（见 §9.5.2） |
| **项目** | 已登记项目的 Skill 使用情况与锁文件状态 | 生成 / 同步 `heron.lock` |
| **更新** | 四组：可升级 / 已固定 / 本地改动 / 上游消失 | 单个或全部升级；差异预览抽屉 |
| **体检** | 问题分组与一键修复 | 修复前展示影响面 |
| **设置** | 路径、默认投影、pin 策略、扫描排除、日志、**语言（中/英）** | 变更即时生效 |
| **原生扫描** | 各客户端的能力与注入状态、写入 diff、还原入口 | 默认关闭，逐客户端确认（§10.11） |

### 12.3 状态语义（全应用必须一致）

| 状态 | 图标 | 含义 |
|------|------|------|
| 生效中 | ● 绿 | 投影在扫描路径内且未被压制 |
| 需重启会话 | ◐ 黄 | 已落地，客户端需重开会话才可见 |
| 未扫描 | ○ 灰 | 落点不在任何客户端扫描路径（白装） |
| 被压制 | ⦸ 红 | 同名且优先级更高者胜出 |
| 有本地改动 | ✎ 蓝 | Store 内容与上游基线不同 |
| 可升级 | ↑ 蓝 | 上游有更新 |
| 冲突 | ⚠ 橙 | 需要用户决策 |
| 坏链 / 漂移 | ✖ 红 | 投影异常 |
| 市场可用 | ◍ 绿 | 适配器体检通过，可检索 |
| 市场不可用 / 已降级 | ◍ 灰 | 接口失败或处于引导式；命中缓存时标注"数据可能过期" |

---

## 13. 用户视角的收拢前后对照

以本机真实情况为例：

```text
收拢前：
  ~/.agents/skills/find-skills/            实体副本 A
  ~/.claude/skills/find-skills  → ../../.agents/skills/find-skills
  ~/projA/.claude/skills/find-skills/      实体副本 B（内容已漂移）

收拢后：
  ~/.heron/skills/find-skills/             唯一实体（含 .git 与 heron.skill.json）
  ~/.agents/skills/find-skills    → ~/.heron/skills/find-skills   (symlink)
  ~/.claude/skills/find-skills    → ~/.heron/skills/find-skills   (symlink)
  ~/projA/.claude/skills/find-skills → ~/.heron/skills/find-skills (symlink)
  <DATA>/backups/<时间戳>/projA.claude.skills.find-skills/      （副本 B 的备份）
```

---

## 14. 里程碑与验收标准

| 里程碑 | 范围 | 完成判据（DoD） |
|--------|------|-----------------|
| **M0 地基** | 工作区、core / platform、扫描与索引、`scan/list/detected`、客户端矩阵（含 native_scan 能力表）、Git preflight 与安装引导、**i18n 骨架（中/英）** | 本机 4 类目录全部识别；扫描性能达标；Windows CI 跑通；两种语言包 key 一致；未装 git 时给出可执行引导 |
| **M1 收拢与分发** | `adopt/project/unproject/targets/verify`、四种投影、Store `~/.heron/skills`、客户端配置只读识别、旧式命令接管、备份与撤销 | 本机 3 处重复 Skill 收拢为 1 份；实体统一存放于 `~/.heron/skills`，投影只写入 `--on` 明确点名的客户端目录；`.claude/commands` 可被收拢且原文件保留；`verify` 能报告客户端加载状态；Windows 普通用户全流程通过 |
| **M2 远程与升级** | `source add/browse`、**五个内置市场适配器**、`add`、`outdated/upgrade/rollback/diff` | 从 GitHub 子目录装 Skill → 本地改一行 → 上游更新 → 三方合并成功 → 回滚；断网或中断不留半态 |
| **M3 体检与团队** | `doctor`（病态嵌套只报告）、原生扫描注入（默认关）、`heron.lock` 导出/导入、`sync` | 同一锁文件在 macOS 与 Windows 两台机器复现同一套 Skill；病态嵌套样本只报告不改动；原生扫描注入前展示 diff 且可一键还原 |
| **M4 GUI 完整化** | 全部页面（Vue 3）、托盘、自动更新、诊断包 | 非技术用户 5 分钟内完成"发现 → 收拢 → 启用"；中英文界面切换无遗漏 |
| **M5 打磨** | 性能、中英双语、崩溃恢复、Linux 试运行 | 冷启动 < 1.5 s；索引损坏可自动重建 |

> 建议 M0–M3 为 **v1.0 发布线**，M4 同版本内完成，M5 进入 v1.1。

---

## 15. 风险与对策

| # | 风险 | 影响 | 概率 | 对策 |
|---|------|------|------|------|
| R1 | **客户端扫描路径 / 优先级随版本变动**，兼容矩阵过期 | 高（投影到错误路径 = 白装） | 高 | 矩阵数据化 + 证据链；`verify` 以真实文件系统复核而非信任表；`copy` / `pointer` 兜底；发布说明列出本次基线版本 |
| R2 | **部分扫描器不识别 symlink / junction**（本机脚本已记录此问题） | 中 | 中 | 投影方式可按目标切换；检测到"疑似未识别"时建议切 `copy` 或 `pointer`；默认不乱铺（按需启用） |
| R3 | **Windows junction 删除误删内容** | **灾难级** | 中 | 唯一入口 `heron-platform::link`；删除前断言 reparse tag 与"目标是链接而非实体"；集成测试覆盖；删除走 `trash` 而非直接抹除 |
| R4 | Rust 与团队现有技能栈不匹配 | 中 | 中 | 内核模块化、`heron-core` 纯逻辑高度可测；前端独立；必要时把界面逻辑下沉到 TS |
| R5 | 每 Skill 一个 Git 仓库导致磁盘膨胀 | 低 | 中 | 浅取、定期 `git gc`、`keep_versions`、`prune_after_days`；对磁盘敏感用户可关闭历史保留（保留最近 N 版） |
| R6 | 大量启用导致上下文膨胀（每 Skill 约 50–100 token） | 中 | 高 | 默认不自动投影；启用前显示预估 token；支持分组启用 |
| R7 | 与 `npx skills`、各客户端 plugin 机制"双头管理" | 中 | 高 | 显式 managed / unmanaged 边界；unmanaged 只读；plugin 拥有的 Skill 标记 `owned-by-plugin` 不接管 |
| R8 | 三方合并对 Markdown 的冲突体验差 | 中 | 中 | 逐块选择 + "保留本地 / 采用上游"快捷策略；二进制文件整文件选择 |
| R9 | 代码签名 / 分发渠道缺失导致安装受阻 | 中 | 中 | 开发期提供未签名构建与安装说明；签名任务前置于发布清单 |
| R10 | 隐私与合规（扫描可能包含敏感信息的项目目录） | 中 | 低 | 永不联网上报；扫描结果仅本地存储；日志脱敏 |
| R11 | **改写客户端原生配置导致客户端异常**（D6 注入） | 高 | 中 | 能力表驱动、默认关闭、逐客户端确认、写入前展示 diff 并备份、`--revert-native` 一键还原；无法确证格式时**宁可不写** |
| R12 | 客户端目录都没开启时，Skill 只在 Store 里、任何客户端都看不见 | 中 | 中 | `project` 不带 `--on` 直接报错并列出可用目标；`verify` 明示"未扫描"；首次引导时询问"是否同时投影到 `.claude/skills` 等客户端目录" |

---

## 16. 评审决议与剩余开放问题

### 16.1 已定稿（2026-11-19 评审）

| 编号 | 决策项 | 结论 | 影响章节 |
|------|--------|------|----------|
| D1 | 默认内置市场 | **全部内置**：skills.sh、clawhub.ai、qoder.com、skillhub.cn 四家默认启用；skillsmp.com 内置但默认关闭。接入方式按各家实测能力分级（见 §9.5） | §7.4 / §9.5 / §18 |
| D2 | GUI 前端框架 | **Vue 3 + TypeScript + Vite + Pinia + Vue Router + Tailwind CSS** | §4 ADR-2 / §5.1 / §6.2 |
| D3 | 项目（Target）发现方式 | **自动发现 + 手动登记并存**：扫描时自动识别项目根，用户也可手动添加 | §7.2 / §8.1 |
| D4 | 国际化 | **第一版即支持简体中文 + English**，i18n 框架在 M0 就位 | §10.10 / §14 |
| D5 | 最低系统版本 | **macOS 12+ / Windows 10 1803+**，接受 | §10.5 |
| D6 | 客户端原生扫描（原 Q2） | **做**：Heron 读取客户端自身配置判断"加载了什么、被谁压制"，**并在用户确认后**将 Store 目录注入为客户端原生扫描目录，作为投影路径漂移的兜底通道 | §10.11 |
| D7 | 无 Git 环境（原 Q3） | **引导安装 Git**；不做静默降级（仅当用户显式选择时才进入"只读预览"模式） | §10.4 / §10.12 |
| D8 | 病态嵌套（原 Q4） | **只报告 + 给修复建议，绝不自动修改**，决定权交给用户 | §8.2 流程 D / §10.13 |
| D9 | 旧式 `.claude/commands`（原 Q5） | **接管**：作为 Skill 的兼容形态纳入管理（导入/收拢/投影），来源标记为 `legacy-command` | §8.3 / §10.14 |
| D10 | `~/.agents/skills` 的定位（原 Q6） | **不作默认落点**：Store 就是 `~/.heron/skills`（Heron 自有），它**不是**投影目标，也没有默认目标；`~/.agents/skills` 仅作为**映射点**（跨客户端共享入口），由用户按需开启 | §7.2 / §10.15 |

### 16.2 剩余开放问题

**无。** 原 Q2–Q6 已于 2026-11-19 全部定稿，见 §16.1 的 D6–D10；原 Q1/Q2（默认市场、前端框架）见 D1/D2。

---

## 17. 术语表

| 术语 | 含义 |
|------|------|
| **Skill** | 含 `SKILL.md` 的目录（Agent Skills 规范） |
| **slug** | Skill 的全局唯一标识（默认取 frontmatter `name` 并规范化；大小写不敏感唯一） |
| **Store** | `~/.heron/skills/<slug>/`，全部 Skill 实体唯一的存放地（受管，含 `.git`）；不是投影目标，不参与扫描（ADR-8 / D10） |
| **Source** | Skill 的来源：Git 仓库 / 本地目录 / 内置市场 |
| **Target** | 落地点：客户端 × 作用域（全局 / 项目） |
| **映射点** | `~/.agents/skills/`，跨客户端共享入口，仅按需开启且与 `npx skills` 共享所有权（D10） |
| **legacy-command** | 旧式命令文件（`.claude/commands/*.md`）接管后的来源标记（D9） |
| **native-scan** | 客户端原生扫描注入能力，能力表驱动、默认关闭（D6） |
| **Projection** | Store → Target 的单向投影（copy / symlink / junction / pointer） |
| **Adopt** | 把散落的未纳管实体收拢进 Store |
| **Managed / Unmanaged** | 是否被 Heron 记录并负责 |
| **Drift** | 目标内容或 Store 内容与记录不符 |
| **Baseline** | 上次成功合并时的上游提交，三方合并的基点 |
| **pin** | 升级策略：exact / semver / none |
| **Visible** | 投影确实会被某个客户端加载（不等于文件存在） |

---

## 18. 参考与证据来源

| 主题 | 来源 |
|------|------|
| Agent Skills 规范（目录结构、frontmatter 约束、渐进式披露、`.agents/skills` 跨客户端约定） | [agentskills.io/specification](https://agentskills.io/specification)、[client implementation guide](https://agentskills.io/client-implementation/adding-skills-support) |
| Claude Code Skill 加载位置、软链接支持、实时监听、保留名 | [code.claude.com/docs/en/skills](https://code.claude.com/docs/en/skills) |
| `marketplace.json` 字段与 plugin source 类型 | [Create a marketplace](https://code.claude.com/docs/en/plugins/create-marketplace)、[Marketplace reference](https://code.claude.com/docs/en/plugins/marketplace-reference) |
| Windows reparse point / junction 删除语义 | [Microsoft Learn: fsutil reparsepoint](https://learn.microsoft.com/en-us/windows-server/administration/windows-commands/fsutil-reparsepoint) |
| schema.org 类型域（用于自建市场的结构化数据） | [SoftwareApplication](https://schema.org/SoftwareApplication)、[SoftwareSourceCode](https://schema.org/SoftwareSourceCode) |
| `npx skills` 锁文件格式 | 本机 `~/.agents/.skill-lock.json`、`~/.agents/skills/skills-lock.json` |
| 内置市场一：skills.sh（目录结构、GitHub 源链接） | [skills.sh](https://www.skills.sh/)；`sitemap-*.xml`（实测 `/<owner>/<repo>/<skill>`） |
| 内置市场二：clawhub.ai（REST API 与能力） | [clawhub.ai](https://clawhub.ai/)；`/api/v1/packages`、`/api/v1/packages/<name>`、`/api/v1/packages/<name>/versions`、`/api/v1/skills`、`/api/v1/search?q=` |
| 内置市场三：qoder.com（能力市场页） | [qoder.com/zh/marketplace](https://qoder.com/zh/marketplace)（实测无公开 API，归为引导式） |
| 内置市场四：skillhub.cn | [skillhub.cn](https://skillhub.cn/)（实测纯 JS 渲染、`robots.txt` 全站 Disallow，归为引导式） |
| 内置市场五：skillsmp.com（默认关闭） | [skillsmp.com](https://skillsmp.com/)（实测返回挑战页） |
| 本机现状实测 | 2026-11-19 实机扫描（§1.3） |

> 说明：**D6 的原生扫描注入能力在实现前必须逐客户端真机验证**（配置文件名、字段、重启要求），结论写入 `clients/*.toml` 的 `native_scan.evidence`；未验证的客户端一律按"不支持"处理。

> 说明：客户端扫描路径与优先级属于**易变事实**，本文矩阵是"基线 + 证据"。实现时必须依靠 `clients/*.toml` 的 `verified_at` 持续复核，禁止把本文当成永久真理。

---

## 19. 评审要点清单（v2.2 · 已通过）

全部 5 条决议（D1–D5）与 5 个开放问题（Q2–Q6 → D6–D10）已定稿，**可转入《实现文档》**。以下为定稿结论复核：

1. 产品形态（§4 ADR-1）与平台范围（§3.1）是否与预期一致？
2. 技术栈（§6）是否接受 Rust + Tauri 2？（这是本设计中最难回溯的决策）
3. 存储模型：Store 独立于 `~/.agents`，并以收拢流程承接历史资产（ADR-3 / ADR-8）是否可接受？
4. 版本方案：每 Skill 独立 Git 仓库 + 本地改动独立分支（ADR-5）是否认可？
5. 四种投影方式（§10.1）的取舍与默认值是否认可？
6. 内置五市场的能力分级（§9.5，A 档一键安装 / B 档引导式）是否认可？
7. 客户端原生扫描口径（§10.11，只读识别 + 能力表驱动的注入）是否符合预期？
8. Store 放在 `~/.heron/skills`（不是投影目标、没有默认目标）与 `~/.agents/skills` 仅作映射点（§10.15）是否符合预期？
