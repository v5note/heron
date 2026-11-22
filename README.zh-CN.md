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

### 4. 图形界面怎么用（macOS · Windows）

界面按「收拢 → 分发 → 维护」三段组织，默认**深色**，左侧栏底部一键切浅色（也可在设置里选）：

| 你在哪里 | 这里是干什么的 |
|---|---|
| **概览** | 只列**现在要你决定的事**：坏链、要合并的、可升级的、没纳管的。没事可做时是一行「没有待处理事项」 |
| **我的 Skill** | 每个 Skill 一行，后面每个客户端一列圆点：实心=生效中，橙=被压制，红=坏链，空心=未被扫描。点开是详情抽屉，可以逐个落点投影/撤掉 |
| **发现** | 磁盘上有、还没纳管的 Skill；勾选后先看 dry-run 预览（只读），确认才移动文件 |
| **来源与市场** | 五个市场：两个可搜索直装，三个只给浏览、回来粘贴地址 |
| **更新** | 本地改过 / 上游更新 / 两边都改过分开显示；冲突进「逐文件合并」，左右并排选，保存后自动重试 |
| **体检** | 只读检查，按错误/警告分级，不动任何文件 |
| **项目** | 客户端真正读取的目录（落点）与项目落点，可直接开关 |
| **设置** | 主题（深/浅）、语言、应用更新、客户端原生扫描能力 |

> 图形界面和命令行共用同一套逻辑，界面上任何改动都会出现在 `heron.lock` 与存储区里。

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

## 开发环境

### 前置工具

| 工具 | 版本 | 说明 |
|---|---|---|
| Rust | stable | `rust-toolchain.toml` 已固定版本，rustup 会自动识别 |
| Node.js | 22+ | 前端需要 |
| pnpm | 10+ | `corepack enable` 即可获得 |
| git | 2.38+ | 三方合并依赖 `git merge-tree --write-tree` |

Linux 上 Tauri 还需要系统库：

```bash
sudo apt install libwebkit2gtk-4.1-dev libgtk-3-dev libayatana-appindicator3-dev librsvg2-dev
```

### 一次性准备

```bash
pnpm --dir ui install     # 前端依赖
cargo build --workspace   # 首次编译 Rust（需要几分钟）
```

> **受限环境 / 只读 HOME：** 本项目所有 Cargo 命令都要求 `CARGO_HOME` 指向可写目录。
> 若 `~/.cargo` 不可写，先 `export CARGO_HOME=$PWD/.cargo-home`——
> `.cargo-home/` 已在 `.gitignore` 中。

### 启动方式

```bash
scripts/dev.sh dev      # 桌面应用，前后端热重载（需 tauri-cli）
scripts/dev.sh web      # 只跑前端，在浏览器里看，没有 Rust 后端
scripts/dev.sh cli …    # 用隔离的数据目录跑命令行
```

**一体化启动桌面应用**（前后端都热重载）：

```bash
cargo install tauri-cli --version '^2' --locked   # 装一次
scripts/dev.sh dev        # 等价于 cargo tauri dev
```

Tauri 会占用 **5183** 端口（strict 模式，端口被占会直接报错而不是悄悄换端口），
Rust 改动会自动重编，窗口加载 `http://localhost:5183`。

**两个进程分开跑**（没装 tauri-cli 时 `scripts/dev.sh dev` 会提示你这么做，
这也是单独调前端最方便的方式）：

```bash
# 终端 1 —— 只跑前端，热重载
pnpm --dir ui dev
# 终端 2 —— Rust 外壳
cargo run -p heron-app
```

**只调前端**（不启动 Rust）：

```bash
scripts/dev.sh web        # 浏览器打开 http://localhost:5183
```

此时调用后端命令会报错（没有 Rust 进程），这是预期的——排版和样式的迭代用这个最快。

**命令行：**

```bash
scripts/dev.sh cli scan                              # 数据目录隔离在 /tmp/heron-dev
HERON_DEV_DATA=/tmp/mine scripts/dev.sh cli list     # 指定隔离目录
cargo run -p heron-cli -- --data-dir /tmp/x scan     # 不用包装脚本
```

### 别碰你真实的 Skill

开发时绝对不能扫到你自己的 `~/.claude`、`~/.codex`。两个隔离开关：

```bash
# 1. 隔离数据目录（索引、存储区、备份一起搬走）
./target/debug/heron --data-dir /tmp/heron-dev scan

# 2. 隔离扫描器看到的家目录
HOME=/tmp/fake-home ./target/debug/heron --data-dir /tmp/heron-dev scan
```

`--data-dir` 会同时接管 data / config / cache / **log** 四类目录，不会往系统路径写任何东西。
在 `/tmp/fake-home` 下造几个假 Skill（`~/.claude/skills/foo/SKILL.md` 之类），
就能安全地演练收拢流程。

### 调试手段

| 场景 | 做法 |
|---|---|
| Rust 日志 | `RUST_LOG=debug cargo run -p heron-app`；命令行部分命令支持 `--verbose` |
| WebView 开发者工具 | 开发版窗口右键 → 检查，或 `cmd+opt+I` / `ctrl+shift+I` |
| 前端调用链路 | 控制台里每次后端调用都是一次 `invoke`，入口在 `ui/src/api/index.ts` |
| 前端报错 | `useAppStore().toasts`——任何失败命令都会以本地化提示弹出 |
| 看索引内容 | `sqlite3 <数据目录>/heron.db '.tables'`、`select * from skill;` |
| 看某个 Skill 的历史 | `git -C <数据目录>/store/<slug> log --oneline` |
| 为什么某个 Skill 不生效 | `heron verify`——区分「未被扫描 / 被压制 / 坏链」 |
| 为什么升级冲突 | `heron conflict <slug>`（加 `--json` 可拿到完整文件内容） |
| 后端 panic | `RUST_BACKTRACE=1 cargo run -p heron-app` |

跑单个测试并看输出：

```bash
cargo test -p heron-service --test remote_flow -- --nocapture
cargo test -p heron-platform --test link_safety_windows -- --nocapture   # 仅 Windows
```

联网用例默认 `#[ignore]`，保证 CI 不依赖外网：

```bash
cargo test -p heron-source --test live_markets -- --ignored --nocapture
```

### 提交前

```bash
scripts/dev.sh check    # fmt + clippy，与 CI 完全一致
scripts/dev.sh test     # 全部测试
scripts/dev.sh ui       # 类型检查 + i18n 一致性 + 构建
```

前端文案只存在于 `ui/src/i18n/locales/{zh-CN,en-US}.json`，两边必须同步——
只在一边有 key 时 `scripts/dev.sh ui` 会直接失败。Rust 侧不产出任何面向用户的句子，
它只返回 `Msg { code, params }`，由界面渲染 `errors.<code>`。

### 固定工具链

`rust-toolchain.toml` 锁定了 Rust 版本，所有人和 CI 用同一个编译器。
如果 `~/.cargo` 不可写，请改 `CARGO_HOME`（见上文），而不是去改版本锁定。

## 从源码构建

```bash
pnpm --dir ui build
cargo build -p heron-cli --release   # 命令行 → target/release/heron
cargo build -p heron-app --release   # 桌面应用
scripts/dev.sh bundle                # dmg / NSIS 安装包（需 tauri-cli）
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
- [发布与签名](docs/SIGNING.md) —— 不依赖付费证书的发布方式
- [决策记录](docs/adr/) —— 实现中踩到的坑与取舍

## 许可

MIT。详见 [LICENSE](LICENSE)。
