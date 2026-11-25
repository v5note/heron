# Heron 项目现状盘点

- 调查时间：2026-11-20 21:00–21:25（Asia/Shanghai）
- 调查方式：只读。未修改源码，未提交，未改变 git 状态；所有命令均在工作区内运行，构建产物写入已忽略的 `target/`。
- 验证环境：macOS 26.4（arm64）；rustc / cargo 1.99.1（stable）；rustfmt 1.10.0-stable；Node 22.22.3；pnpm 10.8.1。
- 标记说明：**【已验证】**＝本次实际运行或可由代码直接证明；**【推测】**＝基于代码阅读的推断，未做运行验证。

## 0. 核心结论

1. **产品骨架已经成形，不是 PPT 项目**：10 个 Rust crate、Tauri 2 桌面壳、Vue 3 界面、5 个客户端矩阵、5 个市场适配器，`v0.1.0` 已打 tag。共 39 个提交，集中在 2026-11-19 至 2026-11-20 两天内完成。
2. **Rust 构建与测试健康**：`cargo test --workspace --all-targets` 共 230 个测试通过、0 失败、3 个联网用例被忽略；`cargo clippy -D warnings` 干净；`heron-cli` 与 `heron-app` 均可构建。
3. **格式检查是当前唯一明确的 CI 红灯**：`cargo fmt --all -- --check` 在 2 个已提交文件上失败，CI 的 `Format` 步骤会拦截。
4. **CLI 主链路可真实跑通**（隔离 HOME 与数据目录实测）：`scan → detected → adopt（含跨目录去重）→ verify → doctor → outdated → lock export`。
5. **主要问题集中在 M3/M4 的“最后一公里”**：Projects 页面的“投影”按钮是 dry-run；项目发现会把 HOME 误注册成项目；项目 target 没有启用/删除入口；`verify` 的 shadowed 状态永远不会出现；`lock apply` 不能复现本地 skill；原生扫描仍是骨架。
6. **文档有若干领先于实现的表述**：`verify` 四态、`doctor` 的 lost sources、`lock` 复现、Projects “each switchable”、所有改动可回滚，均与当前代码存在差距。

---

## 1. 仓库结构

### 1.1 Crate 划分（10 个）

| Crate | 代码行数 | 职责 | 判断 |
|---|---:|---|---|
| `heron-core` | 1,798 | 领域模型：slug、版本、策略、错误码、DTO、哈希；无 IO | 稳定 |
| `heron-platform` | 1,853 | 唯一平台相关层：目录、路径、原子写、锁、链接、探测 | 稳定，含 Windows 链接安全测试 |
| `heron-store` | 1,608 | SQLite schema、迁移、仓储（skill/target/projection/detected/event/source） | 稳定 |
| `heron-scan` | 1,710 | 受控遍历、`SKILL.md` 解析、分类、内容指纹 | 稳定 |
| `heron-vcs` | 1,047 | 每个 skill 一个 Git 仓库、三方合并、版本标签、回滚 | 稳定 |
| `heron-git` | 916 | 远程裸仓缓存、ref 解析、稀疏/子目录检出 | 稳定 |
| `heron-source` | 1,505 | URL 解析、catalog、市场适配器 | 稳定；联网用例默认忽略 |
| `heron-projection` | 983 | 投影 plan / apply / verify，三种投影模式 | 稳定 |
| `heron-service` | 3,686 | CLI 与 GUI 共用的用例层 | 核心用例完整，少数链路半成品 |
| `heron-cli` | 1,117 | clap 命令行、渲染、退出码 | 完整；自身仅 1 个单元测试 |
| `src-tauri` | 447 | Tauri 2 桌面壳，25 个 command，全部委托 service | 薄壳，构建通过 |

Rust 合计约 **16,670 行**。

### 1.2 `src-tauri` 与 `ui`

- `src-tauri/`：Tauri 2 桌面应用 `heron-app`。只有 `main.rs` 和 `lib.rs`，`lib.rs` 注册 25 个 command（`preflight`、`scan`、`detect/adopt/project/unproject`、`verify`、`outdated/upgrade/rollback`、`doctor`、市场、`lock_export/lock_diff`、`projects_discover`、`conflict/diff/resolve`、`native_scan_status/native_scan_plan`），负责把 `Msg` 错误转成前端结构化错误。`cargo build -p heron-app` 通过（32s，增量）。
- `ui/`：Vue 3 + TypeScript + Pinia + vue-router（hash 路由）+ vue-i18n，约 **3,085 行 / 25 个文件**，8 个页面（Overview、Library、Discover、Sources、Projects、Upgrades、Doctor、Settings）、6 个通用组件、1 个 store、3 个 API 文件。`typecheck`、`i18n:check`、`ipc:check`、`build` 均通过。
- `resources/clients/*.toml`：agents、claude、codex、cursor、opencode 五个客户端矩阵，是路径与原生扫描能力的唯一事实来源。
- `scripts/`：`dev.sh`、图标生成、`latest.json` 生成、调试概览。
- `src-tauri/gen/`、`design-demos/`、`releases/` 为辅助/生成/发布物料。

### 1.3 `docs/` 文档清单

| 文档 | 行数 | 内容 |
|---|---:|---|
| `README.md` / `README.zh-CN.md` | 319 / 315 | 产品定位、能力表、CLI/GUI 概览、构建说明 |
| `docs/DESIGN-heron-v2.md` | 1,441 | 完整设计：架构、数据模型、链接安全、投影、升级、市场、GUI |
| `docs/IMPLEMENTATION-heron-v1.md` | 1,461 | 实施计划、里程碑、DoD、滚动进度记录 |
| `docs/GUIDE.md` | 354 | 使用指南、常见问题（含“暂不支持一键撤销收拢”） |
| `docs/REFERENCE.md` | 244 | 命令与行为参考 |
| `docs/SIGNING.md` | 81 | 无付费证书的更新签名方案 |
| `docs/adr/0001`–`0012` | 21–59 / 篇 | 12 个架构决策记录（CARGO_HOME 隔离、链接契约、投影身份、市场与 lock、Windows 链接故障等） |

---

## 2. Git 状态

以调查时快照为准：**【已验证】**

- 分支：`main`，跟踪 `origin/main`，`git rev-list --left-right --count origin/main...HEAD` = `0 0`，**与远端无差异**。
- 提交总数：**39**；时间跨度 `2026-11-19 17:54` → `2026-11-20 20:23`；按日统计 26 + 13。
- 标签：`v0.1.0`。
- 最近提交主题依次为：adopt 残余目录重试修复、图标 master、v0.1.0 双平台文档、release/latest.json 修复、CI runner 修复。
- 工作区**不干净**，调查期间还在持续变化（有另一个会话在改 UI）。以最后一次快照为准：
  - 已修改 6 个文件：`ui/src/App.vue`、`ui/src/i18n/locales/{en-US,zh-CN}.json`、`ui/src/style.css`、`ui/src/views/DiscoverView.vue`、`ui/vite.config.ts`
  - 未跟踪：`ui/src/assets/`（含 `logo-dark.png`、`logo-light.png`、`README.md`；`App.vue` 直接 import 了这两张图）、`scripts/make-rail-logo.py`
  - 本报告目录 `2026-11-20/` 也是新增的未跟踪路径
- 这批 UI 改动是视觉/Discover 页面的在制品，**不是本次调查产生的**。注意：如果提交时漏掉 `ui/src/assets/`，`App.vue` 的 import 会断，CI 的 UI build 会失败。**【已验证：当前状态 + 代码引用】**
- 由于 UI 在制品仍在变化，本文引用的 UI 行号以调查时工作区内容为准；Rust 文件均为已提交版本，引用稳定。

---

## 3. 构建与测试健康

### 3.1 实际执行结果

| 检查 | 命令 | 结果 |
|---|---|---|
| Rust 测试 | `CARGO_HOME=$PWD/.cargo-home cargo test --workspace --all-targets` | **通过**：21 个测试套件，230 passed / 0 failed / 3 ignored，退出码 0 |
| Rust 构建（CLI） | `cargo build -p heron-cli` | **通过**（2m09s，冷依赖） |
| Rust 构建（GUI） | `cargo build -p heron-app` | **通过**（32s，增量） |
| Clippy | `cargo clippy --workspace --all-targets -- -D warnings` | **通过**（4m42s，无警告） |
| 格式 | `cargo fmt --all -- --check` | **失败**，2 个文件有 diff |
| 前端类型 | `pnpm --dir ui typecheck` | 通过 |
| i18n 一致性 | `pnpm --dir ui i18n:check` | 通过 |
| IPC 名称对齐 | `pnpm --dir ui ipc:check` | 通过，25 个 command 两端一致 |
| 前端构建 | `pnpm --dir ui build` | 通过 |

3 个 ignored 用例均为联网市场检查：`skills_sh_live_search`、`clawhub_live_search`、`guided_markets_never_need_the_network`。

### 3.2 格式检查失败详情

`cargo fmt --all -- --check` 退出码 1，涉及两个**已提交且工作区未修改**的文件：

- `crates/heron-service/tests/adopt_flow.rs`
- `src-tauri/src/lib.rs`

典型 diff 是把 `.map(...).unwrap_or_default()` 拆成多行。当前 CI 的 `Format` 步骤正是同一条命令，因此按当前环境判断 **CI 会失败**。**【已验证：本地；推测：CI，取决于 runner 当天拉到的 stable/rustfmt 版本】**

补充：`rust-toolchain.toml` 写的是 `channel = "stable"`，并未固定具体版本，因此“pin 住工具链”的说法不严格成立（`README.md:276`）。

### 3.3 没有验证的部分

- 未执行 `cargo tauri build` / 打包 dmg、NSIS、更新签名；需要 `tauri-cli` 与签名密钥。
- 未运行真实网络的市场搜索与安装。
- 未在 Windows 上运行链接安全门禁（`link_safety_windows`）。
- 未做 5 万目录扫描的性能基准（实现文档的 M0 DoD 指标）。

---

## 4. 文档声称能力 vs 代码实现

| 能力 | 文档声称 | 代码实际 | 结论 |
|---|---|---|---|
| `scan` | 扫描所有已知位置 | walker + 规则 + 解析 + 分类 + 指纹 + 项目根；CLI/GUI 都接 | **完整** |
| `adopt` | 收拢、去重、原位置变链接、先备份 | 备份、store 拷贝、Git 初始化、去重、legacy command、keep-original、失败重试均有；5 个 adopt 专项集成测试 | **完整** |
| `project` / `unproject` | 物化到 target，可移除 | 受管 target 的投影/移除完整；adopt 产生的“原位置投影”不能按客户端 target 移除 | **基本完整，有缺口** |
| `verify` | live / not scanned / shadowed / broken | visible、not scanned、broken 可实现；shadowed 的输入恒为空；restart_required 从不返回 | **部分实现** |
| `doctor` | duplicates、broken links、nested trees、lost sources | 前四类中的 nested/frontmatter/broken/store-missing/duplicate 有；**没有 lost sources**；没有 `--repair` | **部分实现** |
| `install` | 从 git URL / 子目录安装 | 完整，含 URL 形态、GitLab 歧义、bare 缓存、路径穿越防护；本地 bare 远端测试通过 | **完整（仅 git 源）** |
| `outdated` / `upgrade` / `rollback` | 检查、三方合并、回滚 | service + CLI + 本地 bare 远端集成测试均通过；回滚会打 `heron-rollback-backup-*` 标签 | **完整** |
| `diff` / `conflict` / `resolve` | 查看改动/冲突并解决 | 完整，升级冲突后重试有测试 | **完整** |
| `market` | 2 个可搜索 + 3 个仅浏览 | 5 个内置市场；`market list` 实测正常；联网搜索未验证 | **适配器完整，联网未验证** |
| `lock export/diff/apply` | 团队复现 | export/diff 可用；apply 只对 git 源安装，本地源直接报 Missing；targets 只存 kind/scope，无法重建 | **部分实现** |
| `projects` | 发现项目、注册、可切换 | 发现/注册有；默认会把 HOME 注册成项目；target 默认 disabled 且无启用/删除入口；GUI 投影是 dry-run | **半成品** |
| 原生扫描 | 默认关闭，未验证客户端不写 | 只有 status/plan 骨架，status 恒为 absent，plan 使用假路径；apply/revert 无入口 | **骨架** |
| GUI | 8 个页面、25 个 command | 页面齐全、IPC 对齐；Projects、lock、rollback/diff、原生扫描存在空缺 | **主体完成，局部未打通** |

### 4.1 隔离环境端到端演练（已验证）

在 `/tmp` 构造了一个假 HOME（`.claude/skills/wechat-mp`、`.agents/skills/wechat-mp` 同内容，`.codex/skills/foo`，`.claude/commands/review.md`），使用独立 `--data-dir`：

- `scan`：3 个目录、4 ms；识别 3 个 skill、1 个 legacy command。
- `detected`：4 行（wechat-mp 两份、foo、review），review 标记 needs attention。
- `adopt --all`：store 中最终 3 个 skill；两份 wechat-mp 只保留一份实体，两个原位置都变成指向同一 store 的 symlink。
- `verify`：3 行投影均为 `heron:global` / `symlink` / `visible`。
- `doctor`：no issues found。
- `outdated`：三个本地 skill 均为 `not tracked` / `0.0.0`（与文档一致）。
- `lock export --print`：输出 3 个 skill 的 `source: local` 与 `blake3` 内容哈希。

这证明“scan → adopt 去重 → verify”主链路是真实工作的。

---

## 5. 明显问题点

### P1：Projects 页“投影”按钮实际是 dry-run，却提示成功

**位置**：`ui/src/views/ProjectsView.vue:31-41`

```ts
for (const slug of slugs) await api.project(slug, targetId, null, true);
await Promise.all([store.loadTargets(), store.verify()]);
store.toast("ok", t("common.applied"));
```

`api.project` 的第 4 个参数是 `dryRun`；这里恒为 `true`。因此按钮只生成计划，不创建任何链接，随后仍然弹出“已应用”。旁边的注释写着 “Projecting is never faked”，与实现相反。**【已验证：代码直证】**

**影响**：Projects 页的核心操作无实际效果；用户会认为已分发，但 `verify` 看不到变化。

### P1：`projects --register` 会把 HOME 注册成项目，并污染全局 skill 分类

**位置**：`crates/heron-service/src/projects.rs:52-64`

`walk` 先检查当前目录下的 `.<client>/skills` 并直接 `out.push(...)`，之后才执行“HOME 不继续遍历”的保护。由于 HOME 本身通常就有 `.claude/skills` 等目录，默认参数（roots 为空 → HOME）会把 HOME 当成一个项目根。

**实测复现**（假 HOME）：

1. 执行 `heron projects --register`（默认 roots=HOME）。
2. `targets` 出现：
   - `agents:project:af63f34c → ~/.agents/skills`
   - `claude:project:af63f34c → ~/.claude/skills`
   - `codex:project:af63f34c → ~/.codex/skills`
   这些“项目 target”与全局 target 路径完全相同。
3. 再次 `scan`：skill 数从 3 变成 8，全局 skill 被标成 `scope=project`、`client=claude` 等错误归属；`detected` 出现重复行。
4. 后续 scan 会把 HOME 当作项目根继续向下遍历（扫描范围扩大），真实 HOME 下可能触发 `max_dirs` 截断。

**【已验证：运行结果 + 代码】**

**叠加问题**：没有任何 target 启用/禁用/删除入口（`heron-store` 只有 `source::set_enabled`，没有 `target::set_enabled`；CLI/Tauri/UI 均无对应命令）。因此误注册后无法从产品内清理。

### P1：项目 target 默认禁用，但 GUI 无法启用；“each switchable”不成立

- `projects::register` 写入 `enabled: false`（`crates/heron-service/src/projects.rs:106`）。
- 没有启用 target 的 service/CLI/Tauri 命令。
- `LibraryView` 只展示 `enabled === true` 的 target（`ui/src/views/LibraryView.vue:42`），所以项目 target 在“My skills”里永远不出现。
- `ProjectsView` 虽然列出所有 target，但“Project”按钮是 dry-run（见上）。
- CLI 的 `project --on <id>` 不检查 `enabled`，实测可以向 disabled 的项目 target 投影；也就是说能力存在，但 GUI 没有可用路径。

`README.md:81` 写 “each switchable”，与实现不符。**【已验证】**

### P2：`verify` 的四态只实现了三态，shadowed 永远不会出现

- `README.md:24` 声称区分 live / not scanned / shadowed / broken。
- `crates/heron-service/src/verify.rs:97-101` 的 `occupied_names()` 固定返回空 `Vec`，注释写明插件检测要到 M3 才做。
- `crates/heron-projection/src/verify.rs:43-59` 的分类逻辑虽然支持 `Shadowed`，但服务层永远传空列表。
- `RestartRequired` 枚举变体存在，但 `classify` 没有任何分支返回它；`Unknown` 同样不会产生。

**【已验证：代码直证 + 单元测试只覆盖 classify 本身】**

### P2：adopt 产生的原位置链接无法 `unproject`

**实测**：

```
heron unproject wechat-mp --on claude:global
error[E_NOT_MANAGED]
```

原因：adopt 把投影记录挂在默认 target `heron:global` 下（`crates/heron-service/src/adopt.rs:290-292`），但 `unproject` 只匹配指定 target_id 的行；`removed == 0` 时返回 `E_NOT_MANAGED`（`crates/heron-service/src/project.rs:138-159`）。GUI 也没有针对 adopted 原位置的特殊移除路径。**【已验证：运行 + 代码】**

### P2：`lock apply` 不能复现本地 skill，且失败仍返回成功

**实测**：把 fixture 的 lock 导入全新数据目录：

```
foo          Missing
wechat-mp    Missing
review       Missing
installed 0  up to date 0  drift 3
apply exit=0
```

代码在 `crates/heron-service/src/lockfile.rs:228-234` 对 `source == "local"` 直接记 `Missing` 并 `continue`。`README.md:27` 的 “`heron.lock` exports and reproduces it” 只对 git 源成立。另外，diff/apply 在存在 drift 时仍以 0 退出，脚本无法据此判断失败。**【已验证】**

### P2：`doctor` 缺少文档声称的 lost sources，也没有 repair

- `README.md:97` 声称检查 duplicates、broken links、nested trees、**lost sources**。
- `crates/heron-service/src/doctor.rs:44-110` 实际只检查：nested-suspect、frontmatter-invalid、broken-link（仅 `path.exists()`）、store-missing、managed skill 之间的 duplicate-content。
- 没有“上游仓库消失/不可达/来源失效”的检查。
- `docs/IMPLEMENTATION-heron-v1.md:695,1394` 提到 `heron doctor --repair`；CLI 没有该子命令或 flag。

**【已验证：代码 + CLI 参数表】**

### P2：原生扫描仍是骨架，文档暗示“只差一个 supported 开关”

- `crates/heron-service/src/native_scan.rs:46-61`：`status()` 对每个客户端硬编码 `config_path: None`、`state: "absent"`，从不读取真实配置。
- `config_path_for`（`:63-67`）拼的是 `~/.<client>/heron.json`，这是占位路径，不是任何真实客户端的配置格式。
- `apply` / `revert` 存在，但需要外部传入 `enabled`；CLI 没有 native 命令，Tauri 只注册了 `native_scan_status` / `native_scan_plan`（`src-tauri/src/lib.rs:315-327`），UI Settings 只显示 “off”。
- `docs/IMPLEMENTATION-heron-v1.md:1461` 的待办写“真机验证配置格式后把 `native_scan.supported` 改为 true”，但即便改 true，status/写入链路仍不完整。**【已验证：代码；对文档意图的判断为推测】**

### P2：README 的能力表述领先于实现

| README 表述 | 实际 |
|---|---|
| `README.md:14` “Every change can be previewed and rolled back” | 预览主要覆盖 adopt/project/upgrade；回滚只覆盖 upgrade；adopt/project 没有产品内撤销命令（`docs/GUIDE.md:343-344` 也承认“暂不提供一键撤销收拢”） |
| `README.md:24` verify 区分 shadowed | shadowed 输入恒为空 |
| `README.md:27` lock 可复现技能集 | 本地来源无法复现 |
| `README.md:81` Projects 的 target“each switchable” | 无启用开关 |
| `README.md:97` doctor 检查 lost sources | 未实现 |
| `README.md:276` “rust-toolchain.toml pins the Rust version” | 只固定 `stable` 通道，未固定版本 |

### P3：扫描截断时会误删索引行

`crates/heron-service/src/scan.rs:81-119` 在持久化时无条件 `prune_not_in`；而 `ScanOptions.max_dirs` 默认 50,000（`crates/heron-scan/src/walker.rs:27-36`），截断后未遍历到的目录不会被 `seen` 收录，其 detected 行会被删除。真实 HOME 误注册为项目根后更容易触发。**【推测：基于代码路径，未构造 5 万目录实测】**

### P3：死代码与未使用接口

- 占位死代码：`crates/heron-service/src/install.rs:352`、`crates/heron-scan/src/fingerprint.rs:183`、`src-tauri/src/lib.rs:435` 的 `#[allow(dead_code)] fn _p/_assert_pathbuf/_msg`。
- `native_scan::apply/revert` 无调用方。
- `verify::scan_paths_for` 的 `targets` 参数被 `let _ = targets;` 丢弃（`crates/heron-service/src/verify.rs:82`）。
- UI API 层暴露但页面从不调用：`rollback`、`diff`、`lockDiff`、`lockExport`、`nativeScanPlan`（`ui/src/api/index.ts:57,67-72,76`）。因此 GUI 没有 rollback、lock、diff 页面能力。
- `PinPolicy::Exact/None` 只在 core 测试中出现；CLI 固定使用 `Semver`，`UpgradeState::Pinned` 在产品路径不可达。

### P3：测试覆盖空缺

- 共 230 个测试，数量健康，但集成测试只有 `adopt_flow`（11 个）和 `remote_flow`（6 个），其余以 crate 内单元测试为主。
- 没有 CLI 端到端测试；本次手工跑了隔离 HOME 流程。
- 没有 `lock` 的 diff/apply 行为测试（只有序列化 roundtrip）。
- 没有“projects 默认扫描 HOME”的回归测试；现有 `projects` 测试只传入子目录作为 root，恰好绕过了该缺陷。
- 没有前端单元/组件测试；UI 只有 typecheck、i18n、IPC 名称、build 四道静态门禁。
- `doctor` 只有 nested-suspect 的集成用例；lost sources 本就不存在。
- 原生扫描只有 4 个单元测试，无 CLI/Tauri 端到端用例。

### P3：当前 UI 在制品本身的小问题

- `ui/src/views/DiscoverView.vue` 的工作区改动里，表格首列已经渲染了一次 kind chip，后面可能还存在独立“kind”列，存在重复展示的风险。**【推测：基于 diff 阅读，未在浏览器渲染验证】**
- 未跟踪的 `ui/src/assets/` 被 `App.vue` import，提交时需一并纳入。

---

## 6. TODO / FIXME 与死代码扫描

- 在自有源码（排除 `node_modules/`、`src-tauri/gen/`、`target/`）中：**TODO / FIXME / XXX / HACK / todo! / unimplemented! 数量为 0**。项目更倾向于用明确的占位函数和注释表达未完成项。
- 明确的“未完成占位”集中在：
  - `heron-service/src/verify.rs` 的 `occupied_names`（注释：M3 再做）；
  - `heron-service/src/native_scan.rs` 的 status/config_path；
  - 三个 `#[allow(dead_code)]` 空函数。
- 明确的“未打通链路”：
  - ProjectsView 的 dry-run 投影；
  - target 的启用/删除；
  - lock 的本地源复现；
  - GUI 的 rollback/diff/lock/原生扫描写入。

---

## 7. 建议的下一步优先级（供参考）

1. **修 P1 断点**：ProjectsView 传 `dryRun=false`；`projects::discover` 跳过 HOME 本身；补 target 启用/禁用/删除入口，并补对应回归测试。
2. **让格式门禁恢复绿色**：对两个文件执行 `cargo fmt` 后提交，避免 CI 在 Format 步骤失败。
3. **补齐 verify 与 doctor 的承诺**：要么实现 plugin 占位检测与 lost-source 检查，要么先把 README 的四态/lost sources 表述降级。
4. **明确 lock 边界**：在 README 标注“本地来源只记录哈希、不负责跨机复现”，并让 `lock apply` 在有 drift 时返回非 0（或提供 `--allow-drift`）。
5. **原生扫描要么补齐写入链路，要么在文档中明确为未实现原型**；当前“改 supported=true”不足以让它工作。

---

## 8. 结论

- **健康度**：Rust 代码质量与测试基础扎实（230 测试、clippy 全绿、CLI/GUI 均可构建），唯一明确的工程门禁问题是 `cargo fmt --check`。
- **完成度**：M0–M2 的核心（扫描、收拢、投影、Git 安装、三方升级、回滚、diff/conflict）是真实可用的；M3/M4 的项目管理、lock 复现、原生扫描、GUI 部分交互仍是半成品或骨架。
- **最大风险不是“没写”，而是“看起来能用但实际没生效”**：Projects 页 dry-run 提示成功、HOME 被注册成项目、disabled target 无启用入口、lock apply 静默 Missing，这四处最容易让用户对产品状态产生误判。
