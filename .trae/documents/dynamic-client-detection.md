# 动态扫描本机客户端（presence detection）

## Context

概览页「落点状态」目前固定渲染 5 个客户端：`BUILTIN_FILES` 用 `include_str!` 把 5 个 TOML 编译进二进制（[clients.rs#L15-36](file:///Users/jingyuan-mb01/javaspace/REA/heron/crates/heron-service/src/clients.rs#L15-L36)），`bootstrap()` 被 kv `bootstrap_done` 锁死只跑一次。全仓库唯一的"存在性"概念是扫描时 `if !root.exists()` → `roots_missing`（[walker.rs#L201](file:///Users/jingyuan-mb01/javaspace/REA/heron/crates/heron-scan/src/walker.rs#L201)），它只是个报告字段，从不落到 per-client 状态。

由此产生两个问题：

1. **chip 语义说谎**。"未被扫描"实际是"该落点上没有生效投影"的派生结果（[OverviewView.vue#L229-235](file:///Users/jingyuan-mb01/javaspace/REA/heron/ui/src/views/OverviewView.vue#L229-L235)），跟"这台机器装没装 Cursor"无关。用户无法区分"没装"和"装了但没投"。
2. **前端已经认识 13 个客户端，后端只注册 5 个**。[clients.ts#L6-36](file:///Users/jingyuan-mb01/javaspace/REA/heron/ui/src/utils/clients.ts#L6-L36) 有 trae/qoder/grok/cline/copilot/workbuddy/antigravity 的图标与品牌色，[DiscoverView.vue#L79-82](file:///Users/jingyuan-mb01/javaspace/REA/heron/ui/src/views/DiscoverView.vue#L79-L82) 的 `COMMON_CLIENTS` 列了 12 个 —— 这些客户端装了也不会出现在落点里。

目标：扫描本机时按声明式规则探测客户端是否安装，客户端目录从 5 扩到 12，未检测到的保留卡片但灰显 + 新 chip「本机未检测到」，不删除任何行。

## 已定决策

| 问题 | 决策 |
|---|---|
| 范围 | 扩表 + 按检测建落点 |
| 探测信号 | TOML 声明的混合信号（目录 / CLI `--version` / macOS `.app`），任一命中即算检测到 |
| 未检测到 | 保留但灰显 + 新 chip；**绝不删行、绝不动 `enabled`** |
| 目录来源 | 保持编译内置（`tauri.conf.json` 的 `resources: []` 不改） |

核心不变量：

- **`detected` 与 `enabled` 正交**。`enabled` 是用户开关（且目前**没有任何命令能把它设回 true**），检测永不写它。UI 判定 `live = enabled && detected !== false`。
- **三态 `detected`**：`NULL` 从未检测（升级后首扫前，渲染如旧）／`0` 检测过未命中（灰显）／`1` 命中。避免升级瞬间满屏灰卡。
- **检测不遮蔽扫描器**：`collect_roots` 仍遍历全部客户端目录。否则误判为"未装"的客户端永远没有翻身的机会（唯一能纠正它的信号就是一次扫描）。

## 探测规则的证据（2026-11-23 查证）

7 个新客户端**都**支持 SKILL.md 约定。`global_skills` 一律取官方文档值；`project_skills` 允许推断（它只影响分类，不影响写入）。

| id | global | project | 主要探测信号 | 来源 |
|---|---|---|---|---|
| trae | `.trae/skills` | `.trae/skills` | `~/.trae` | docs.trae.ai/ide/skills |
| qoder | `.qoder/skills` | `.qoder/skills` | `~/.qoder` | docs.qoder.com/extensions/skills（`rescan="session"`） |
| grok | `.grok/skills` | `.grok/skills` | `~/.grok`、`grok` | docs.x.ai/build/features/skills-plugins-marketplaces |
| cline | `.cline/skills` | `.cline/skills` | `~/.cline` | docs.cline.bot/customization/skills |
| copilot | `.copilot/skills` | `.github/skills` | `~/.copilot` | code.visualstudio.com + docs.github.com |
| antigravity | `.gemini/config/skills` | `.agents/skills` | `~/.gemini`、`agy` | antigravity.google/docs/skills |
| workbuddy | `.workbuddy/skills` | `.workbuddy/skills`（推断） | `WorkBuddy.app`、`~/.workbuddy` | workbuddy.ai/docs/cli/skills + 本机实测 |

两处刻意不做：

- **Trae CN 不单独收录**。本机跑的是 Trae CN（`~/.trae-cn/` 存在），但 `~/.trae-cn/skills` 只有火山引擎文档和 trae 论坛在说，docs.trae.ai 只承认 `~/.trae/skills`。把它当真理写进去会让 Heron 往一个 CN 版可能不读的路径投影。结果：这台机器上 trae 会显示"本机未检测到"——这是**诚实的**。要修就得真机验证（往 `~/.trae-cn/skills` 放一个 skill 看 CN 版认不认），列为后续项。
- **Antigravity 不写 `.app` 名**。官方从未公布 bundle 文件名，只留 `~/.gemini` 目录信号。

## 实施步骤

### 1. Store：presence 三列（无行为变化）

新增 `crates/heron-store/src/migrations/0005_client_presence.sql`（仿 `0002_client_commands.sql` 风格，注释写明"为什么"）：

```sql
ALTER TABLE client ADD COLUMN detected INTEGER;
ALTER TABLE client ADD COLUMN detected_at TEXT;
ALTER TABLE client ADD COLUMN detect_evidence TEXT NOT NULL DEFAULT '[]';
```

- 在 [db.rs#L10-21](file:///Users/jingyuan-mb01/javaspace/REA/heron/crates/heron-store/src/db.rs#L10-L21) 的 `MIGRATIONS` 追加，`SCHEMA_VERSION` 自动变 5。
- `ClientRow`（[models.rs#L6-19](file:///Users/jingyuan-mb01/javaspace/REA/heron/crates/heron-store/src/models.rs#L6-L19)）加 `detected: Option<i64>` / `detected_at: Option<String>` / `detect_evidence: Vec<String>`。
- `repos/client.rs`：`list` 的 SELECT 补三列；新增 `set_presence(tx, id, detected: Option<bool>, at, evidence)` 走窄 `UPDATE`。
- **`upsert` 一列都不加**，只在上面写注释说明"presence 有独立写入路径，两者永不互相覆盖"。这是整套设计的关键：重跑 bootstrap 不会重置检测态，写检测态不会重置目录表。
- 全仓库 `ClientRow { … }` 字面量随之编译失败——这是故意的爆炸半径探针，逐个补 `None/None/vec![]`。

### 2. Platform：可超时的进程探测

[probe.rs](file:///Users/jingyuan-mb01/javaspace/REA/heron/crates/heron-platform/src/probe.rs) 现有 `detect_git` 用 `Command::output()`，**没有截止时间**：一个卡住的 `--version`（TUI 等 TTY、首启 OAuth、npm shim 慢缓存）会永久占住 `spawn_blocking` 线程，`ScanState.running` 恒真，GUI 卡在"扫描中"。

- 新增 `pub fn cli_version(bin: &str, args: &[&str], timeout: Duration) -> Option<String>`：piped stdout/stderr + 两个读线程 + 轮询 `try_wait()`，超时 `kill()` 后 `wait()`（kill 会关管道让读线程退出）。仅"在截止时间前退出 0"才返回 `Some`。
- 新增 `pub fn which(bin: &str, extra_dirs: &[PathBuf]) -> Option<PathBuf>`：`env::split_paths(PATH)` 加候选目录（`/opt/homebrew/bin`、`/usr/local/bin`、`~/.local/bin`、`~/.cargo/bin`、`~/.bun/bin`）。**必要性**：Finder 启动的 macOS GUI 只有 `/usr/bin:/bin:/usr/sbin:/sbin`，npm/brew 装的 `claude`/`codex` shim 裸 `Command::new` 永远找不到——这是系统性误判。
- `detect_git` 改为基于 `cli_version` 重写，签名不变。
- 单测：`#[cfg(unix)]` 用 `/bin/sleep 5` + 200ms 截止断言返回 `None` 且远小于 1s。

### 3. 目录扩表 + `[detect]` schema

- `clients.rs` 加 `DetectToml { dirs: Vec<String>, commands: Vec<CommandProbeToml>, apps: Vec<String> }`、`CommandProbeToml { bin, args, timeout_ms }`，`ClientToml` 加 `#[serde(default)] detect: Option<DetectToml>`（与 `native_scan`/`rescan` 同风格）。
- `builtin()` 加 id 唯一性校验。
- 5 个现有 TOML 补 `[detect]`；新增 7 个 TOML；`BUILTIN_FILES` 补 7 条。
- 路径规则写进模块注释：`global_skills`/`project_skills` 仍是相对 home / 相对仓库根；`detect.dirs` 的 `~` 用 `paths::expand_tilde`，其余**按字面**（允许绝对路径）；存在性用 `is_dir()`（symlink 目录算命中）。`detect.apps` 只在 macOS 查 `/Applications` + `~/Applications`，**其他平台是"跳过"而非"失败"**。
- 顺带修两处扩表后的隐性回归：
  - [classify.rs#L205](file:///Users/jingyuan-mb01/javaspace/REA/heron/crates/heron-scan/src/classify.rs#L205) `is_nested_suspect` 硬编码 `CLIENTS: &[".claude", ".agents", ...]` → 改为在 `Classifier::new` 里从 `project_paths` 首段推导，删掉常量。否则 7 个新客户端的病态嵌套检测静默失效。
  - [native_scan.rs#L47](file:///Users/jingyuan-mb01/javaspace/REA/heron/crates/heron-service/src/native_scan.rs#L47) `status()` 遍历 `builtin()` → 改为只列声明了 `[native_scan]` 的客户端，否则设置页多出 7 行噪声。

### 4. Service：`detect.rs` + `sync_catalog`

新增 `crates/heron-service/src/detect.rs`（`lib.rs` 注册 `pub mod detect;`），不碰 DB：

```rust
pub trait Machine {
    fn is_dir(&self, p: &Path) -> bool;
    fn app_exists(&self, name: &str) -> bool;   // 非 macOS 恒 false，由 supports_app_bundles 决定语义
    fn cli_version(&self, bin: &str, args: &[&str], timeout: Duration) -> Option<String>;
    fn now(&self) -> String;
}
pub fn run(matrix: &[ClientToml], home: &Path, m: &dyn Machine, o: &DetectOptions) -> Detection;
```

- **按 `dirs → apps → commands` 短路**：前两个免费，进程探测才花钱。命中即停，`evidence` 记 `["dir:~/.claude"]` 之类，供 tooltip 与排查误判。
- `detect` 缺失或三段全空 ⇒ `detected = None`（未知），永不返回"已检测到"。
- `detect::run` 不可失败（返回 `Detection` 而非 `Result`）：探测报错 = 该信号未命中。
- `clients.rs` 拆出 `sync_catalog(db, home, force: bool)`：**只插不改**（`force=true` 保留 bootstrap 首跑的全量 upsert 语义）。`bootstrap()` 签名不变，body 变 `if done { return Ok(()) } sync_catalog(force=true)?; kv_set(...)`。
- `apply_presence(db, det)`：只做 `set_presence` 的窄 UPDATE，**不建 target**（client 行的唯一创建者是 `sync_catalog`，避开 `target.kind` 的 FK 顺序问题）。
- `pub fn target_live(t: &TargetRow, clients: &[ClientRow]) -> bool` —— 全仓库唯一定义点。

### 5. 接进扫描

`scan::prepare` 内顺序：`sync_catalog(force=false)` → `detect::run` → `apply_presence` → `loaded_specs` → `Classifier::new` → `collect_roots`。检测在 classifier 之前，本轮新检测到的客户端本轮就能被扫到。

- 加 `prepare_with(ctx, home, req, machine: &dyn detect::Machine)`，`prepare` 保持原签名薄委托 `RealMachine`。这样 [lib.rs#L273](file:///Users/jingyuan-mb01/javaspace/REA/heron/src-tauri/src/lib.rs#L273)、[app.rs#L211](file:///Users/jingyuan-mb01/javaspace/REA/heron/crates/heron-cli/src/app.rs#L211) 和各测试零改动。
- `apply_presence` 的错误记进日志后吞掉：DB 忙不能让用户原本能成功的扫描失败。
- 不改 `ScanReport`/`ScanDoneDto`/`summarize`：presence 是 DB 里的持久状态，事件总线只管进度，多一份 DTO 就多一个会和网格打架的东西。

### 6. Tauri

- 新增 `#[tauri::command] fn clients_list(...) -> Vec<ClientRow>`（[lib.rs#L500](file:///Users/jingyuan-mb01/javaspace/REA/heron/src-tauri/src/lib.rs#L500) 旁），注册进 `generate_handler!`。没有它概览无法把 presence join 到 target 行上。
- 启动处（[lib.rs#L889](file:///Users/jingyuan-mb01/javaspace/REA/heron/src-tauri/src/lib.rs#L889)）`bootstrap` 之后补 `sync_catalog(force=false)`，让升级带来的新 TOML 在首扫前就有行。

### 7. 前端

- `api/types.ts` 加 `ClientRow`；`api/index.ts` 加 `clients: () => call<ClientRow>("clients_list")`。
- `stores/app.ts`：`clients` state + `loadClients()`（照 `loadTargets()`）进 `refreshAll()`；getter `clientById`、`isLiveTarget(t)`（同三态规则）；`applyProgress` 的 `phase === "prepare"` 与 `finishScan` 各补一次 `loadClients()+loadTargets()`——**`finishScan` 现在根本不重载 targets**（只有 `adopt()` 会），扩表后这是必须修的真缺口，不是美化。
- `OverviewView.vue`：`targetCards` 在 chip 链**最上方**插 `detected === false → chipAbsent`（presence 优先于 broken/live），返回对象加 `detected`；模板加 `:class="{ absent: ... }"` 与 `title`；CSS `.tile.absent { opacity:.55 }` + 只给 `.t-glyph` 加 `filter: grayscale(1)`（chip 文字必须保持可读）；`enabledTargets` 改用 `isLiveTarget`，让落点计数与灰显口径一致。
- **不动** `VisibilityState`、`stateOf`、visibility 表 CHECK：新 chip 是 target chip，不是投影可见性状态。
- `LibraryView.vue#L66`、`ProjectsView.vue#L50` 的 `t.enabled` → `store.isLiveTarget(t)`；`DiscoverView.vue` 的 `COMMON_CLIENTS` 改为优先取 `store.clients`。
- i18n 两份都改，键放 `overview.targets.*`：`chipAbsent` = 「本机未检测到」/「Not found on this machine」，`absentHint` = 「未在本机发现该客户端；重新扫描会自动恢复，落点记录未被删除」。
- `clients.ts` 无需改动（13 个 id 已齐，未知 id 安全降级）。

### 已核实为无需改动

`lockfile::export`（[lockfile.rs#L85-92](file:///Users/jingyuan-mb01/javaspace/REA/heron/crates/heron-service/src/lockfile.rs#L85-L92)）虽然遍历全部 target 行，但内部 `if target.scope == "project"` 已经过滤掉全局落点 —— 扩表不会改变 lockfile 输出，跨机器 diff 不受影响。

## 验收

```bash
cargo fmt --check && cargo clippy --workspace -- -D warnings
cargo test --workspace
cd ui && npm run typecheck && npm run build
```

必过的新增断言：

- `detect.rs`（`FakeMachine`）：三段信号任一命中／全不命中→`Some(false)`／规则空→`None`；**`dirs` 命中时断言 `cli_calls` 为空**（短路是性能契约）；非 macOS 时 apps 信号是跳过不是失败；`~` 展开；evidence 文案。
- `clients.rs`：`sync_catalog` 幂等（连调两次不新增）；手工改过 `display_name` 后重跑不被覆盖；`bootstrap_done=1` 之后目录新增客户端能补出 client+target 行（升级路径）；`set_presence` 后 `bootstrap(force=true)` 仍保留 `detected=0`（反方向覆盖测试）；`target_live` 的 `enabled × {None,Some(0),Some(1)}` 真值表；每个内置 TOML 都有非空 `detect`。
- 集成 `crates/heron-service/tests/detect_scan_flow.rs`：临时 home 只造 `~/.codex`，`prepare_with` + FakeMachine，断言 codex `detected=1` 带证据、claude `detected=0`，且 **roots 仍包含未检测客户端的全局目录**（把"检测不遮蔽扫描器"钉住）。
- 存量断言要改：`clients.rs` 的 `len()==5` 三处、`native_scan.rs:248`、`store_integration.rs` 的 `version != 4` → 5 并补 `detected.is_none()`。`heron-scan/tests/scan_integration.rs` 自建 `ClientSpec`，不受扩表影响（已核实）。

手工验证（本机即 Trae CN + Qoder + Grok 环境）：`cargo tauri dev` → 点「扫描本机」→ 概览应出现 12 张卡；grok/qoder/claude/codex/agents 绿 chip，cursor/opencode 视本机而定，trae/antigravity 灰显「本机未检测到」且悬停有 hint；设置页原生扫描表不再多出无 `[native_scan]` 的行。

## 风险与后续

- **R1 macOS GUI PATH 误判**（影响最大）：靠 `which` 候选目录 + 目录/`.app` 作首要信号缓解；误判时 evidence 进 tooltip，用户能看出是"没探测到"而非"被卸载了"。
- **R2 新鲜度**：检测只在扫描时发生，扫完再装的客户端要等下次扫描。可选缓解：设置页刷新按钮走一次纯检测。
- **R3 用户想覆盖"未检测到"往它投影**：目前**没有任何命令能改 `target.enabled`**（概览那个 switch 是无 handler 的 `<span>`）。真要开放覆盖需新增 `target_set_enabled`，与 `detected` 是两个概念，别混。
- **后续项**：真机验证 Trae CN 的 `~/.trae-cn/skills` 与 workbuddy 项目级路径，验证通过后补 `trae-cn` 客户端与 `verified_by="probe"`。
- 本方案第一次让 Heron 在 `git` 之外执行外部二进制，`log_action` 不记录它——建议 PR 描述里明确写出探测范围，别让它成为意外。
