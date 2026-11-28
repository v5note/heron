# 事实核查表（facts.md）— Heron 苍鹭

> 所有事实均直接核验自本仓库源码、`Cargo.toml`、`README.zh-CN.md`、`website/index.html` 与 `images/` 真实运行截图，无任何编造数据。

| 核查项 | 核实值 | 来源 |
|---|---|---|
| 项目名称与版本 | Heron 苍鹭 · `v0.1.0` | `Cargo.toml` (`version = "0.1.0"`) |
| 开源协议 | MIT License | `LICENSE` / `Cargo.toml` |
| 官网地址 | `https://heron.v5ai.xin` | 用户提供 + `website/` |
| 源码仓库 | `https://github.com/v5note/heron` | `README.zh-CN.md` / `website/index.html` |
| 技术栈 | Rust 1.80+ (edition 2021)、Tauri 2、Vue 3 + TypeScript、SQLite (`rusqlite` bundled)、Blake3、rayon | `Cargo.toml` / `README.zh-CN.md` |
| 安装包体积与平台 | 约 5–15 MB 原生体积，零运行时依赖；官方包提供 macOS (Apple Silicon / Intel `.dmg`) 与 Windows x64 (`.exe`)，内核支持 Linux | `website/index.html` / `README.zh-CN.md` |
| 环境前置依赖 | Git 2.38+（三方合并需要 `git merge-tree --write-tree`）；macOS 12+ 或 Windows 10 1803+ | `README.zh-CN.md` |
| 签名说明 | 采用 Tauri 内置 minisign 更新签名（未做付费系统级代码签名），首次打开 macOS 需右键打开，Windows 需点「仍要运行」 | `README.zh-CN.md` / `docs/SIGNING.md` |
| 内置客户端数量 | 13 款（Claude Code、Codex、Cursor、Trae、Trae CN、Qoder、WorkBuddy、Cline、GitHub Copilot、Grok Build、Antigravity、opencode、`.agents`） | `README.zh-CN.md` / `resources/clients/` |
| 投影模式 | 4 种：`symlink`、`junction`（Windows 免提权）、`pointer`（代理 `SKILL.md`）、`copy`（含 `E_DRIFTED` 漂移防护） | `README.zh-CN.md` / `website/index.html` |
| 内置市场数量与分级 | 6 大市场：可搜索直装（`skills.sh`、`ClawHub`）；浏览器引导（`Qoder`、`SkillHub`、`ModelScope`、`SkillsMP`，其中 ModelScope 支持粘贴链接克隆安装） | `README.zh-CN.md` / `images/来源与市场-浅色.png` |
| 真实源与版本控制 | `~/.heron/skills/<slug>`，每个 Skill 为独立 Git 仓库，含 `main`、`upstream` 分支与 `v*` 标签，回滚前自动打 `heron-rollback-backup-*` 保护标签 | `README.zh-CN.md` |
| 团队锁文件 | `heron.lock`（不含绝对路径），支持 `heron lock export` / `diff` / `apply` | `README.zh-CN.md` |
| 内置主题 | 4 套配色主题（默认、暮色、林墨、晚晴），支持自动/浅色/深色模式 | `README.zh-CN.md` / `website/index.html` |
| 安全契约 | 先备份再动文件（`<数据目录>/backups/`）；拒绝隐式覆盖（`E_NOT_MANAGED`）；`copy` 模式漂移保护（`E_DRIFTED`）；只读体检（`heron doctor`） | `README.zh-CN.md` |
