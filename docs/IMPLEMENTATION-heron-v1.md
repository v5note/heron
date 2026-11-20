# 苍鹭 Heron · 实现文档 v1

> **文档编号**：heron-IMPL-001  
> **状态**：可执行（设计依据：[DESIGN-heron-v2.md](DESIGN-heron-v2.md) v2.2，含决议 D1–D10）  
> **日期**：2026-11-19  
> **目标读者**：负责落地第一版（v1.0）的研发  
> **配套**：设计文档定义"做什么/为什么"，本文定义"怎么落地/按什么顺序/如何验收"  
> **使用文档**：[README](../README.md) · [上手指南](GUIDE.md) · [命令参考](REFERENCE.md)

---

## 0. 本文如何使用

| 你的角色 | 阅读方式 |
|----------|----------|
| 主程 / 架构 | §1 工程结构 → §3 领域接口 → §4 平台层 → §11 实施顺序 |
| 后端（Rust） | §2 数据库 → §3/§4/§5 接口 → §6 市场适配 → §11 顺序 |
| 前端（Vue） | §7 命令面 → §8 前端结构 → §9 i18n |
| 测试 / 质量 | §10 测试矩阵 → §12 验收清单 |
| 运维 / 发布 | §1.4 构建与 CI → §13 发布 |

**硬性约束**（贯穿全文，违反即返工）：

1. `heron-core` 零 IO、零 async、零系统时间；纯逻辑可用 `proptest` 穷举。
2. 只有 `heron-platform` 允许出现 `#[cfg(windows)]` / `#[cfg(unix)]`。
3. 所有面向用户的文案以 `{ code, params }` 结构化返回，**Rust 代码中不得出现 UI 文案字符串**（§9.2）。
4. 任何"删除链接/投影"的动作必须走 `platform::link`，禁止直接 `fs::remove_dir_all`（设计 §10.2 / 风险 R3）。
5. 任何写操作必须先在 `event` 表登记补偿信息（§4.7 事务模型）。

---

## 1. 工程结构

### 1.1 仓库布局（Cargo workspace + pnpm workspace）

```text
heron/
├── Cargo.toml                     # [workspace] 聚合 crates/*
├── rust-toolchain.toml            # 钉死 stable 版本
├── rustfmt.toml / clippy.toml
├── package.json                   # pnpm workspace 根
├── pnpm-workspace.yaml
├── src-tauri/
│   ├── Cargo.toml                 # crate: heron-app（Tauri 壳）
│   ├── tauri.conf.json
│   ├── capabilities/
│   │   ├── main.json              # 窗口能力声明
│   │   └── cli.json               # CLI 需要的能力
│   ├── icons/
│   └── src/
│       ├── main.rs                # #[cfg_attr(mobile, tauri::mobile_entry_point)]
│       ├── lib.rs                 # run()，供测试与 CLI 复用
│       ├── commands/              # Tauri command 注册（见 §7）
│       │   ├── mod.rs
│       │   ├── discover.rs
│       │   ├── library.rs
│       │   ├── project.rs
│       │   ├── sources.rs
│       │   ├── upgrade.rs
│       │   ├── doctor.rs
│       │   ├── lockfile.rs
│       │   └── system.rs
│       ├── events.rs              # 进度事件发射
│       └── state.rs               # AppState（Service + 取消令牌）
├── crates/
│   ├── heron-core/                # 纯领域
│   │   ├── Cargo.toml
│   │   └── src/
│   │       ├── lib.rs
│   │       ├── slug.rs            # Slug 规范化与平台合法性
│   │       ├── frontmatter.rs     # 模型（解析实现在 heron-scan）
│   │       ├── version.rs         # SemVer + 0.0.0+sha 兜底
│   │       ├── policy.rs          # PinPolicy / ConflictPolicy / ProjectionMode
│   │       ├── hash.rs            # 规范化哈希算法定义（纯函数签名）
│   │       ├── error.rs           # ErrorCode + Msg{code, params}
│   │       └── ids.rs             # TargetId / SourceId / MarketId 形态与校验
│   ├── heron-platform/            # 唯一平台分支点
│   │   ├── src/
│   │   │   ├── lib.rs
│   │   │   ├── dirs.rs            # 数据/配置/缓存/日志目录解析
│   │   │   ├── paths.rs           # 长路径前缀、相对化、规范化、展示转义
│   │   │   ├── link/              # mod.rs + unix.rs + windows.rs（§4）
│   │   │   ├── lock.rs            # 单实例与文件锁（fs4）
│   │   │   ├── atomic.rs          # 原子写（临时文件 + rename + fsync）
│   │   │   ├── process.rs         # 子进程封装（git 等，超时/取消/kill tree）
│   │   │   └── probe/             # 平台探测（git 版本、WebView、OneDrive 等）
│   ├── heron-scan/                # 遍历 + 解析
│   │   ├── src/
│   │   │   ├── lib.rs
│   │   │   ├── walker.rs          # 受控遍历（深度/数量/忽略/防环）
│   │   │   ├── rules.rs           # 忽略规则匹配（globset）
│   │   │   ├── parse.rs           # SKILL.md frontmatter（两级解析）
│   │   │   ├── classify.rs        # 判定 client/scope/project/legacy-command
│   │   │   └── fingerprint.rs     # 规范化内容哈希（blake3）
│   ├── heron-store/               # SQLite 仓储
│   │   ├── src/
│   │   │   ├── lib.rs
│   │   │   ├── db.rs              # 连接池、PRAGMA、迁移入口
│   │   │   ├── migrations/        # 0001_init.sql …（§2.3）
│   │   │   └── repos/             # skill/target/projection/source/detected/event
│   ├── heron-vcs/                 # 每 Skill Git 仓库
│   │   ├── src/
│   │   │   ├── lib.rs
│   │   │   ├── repo.rs            # init / commit / branch / tag
│   │   │   ├── sync.rs            # 与远端交互（通过 heron-git 的 bare 缓存）
│   │   │   ├── merge3.rs          # 三方合并（git merge-tree 优先，similar 兜底）
│   │   │   └── snapshot.rs        # 用户显式选择"只读预览"时的快照模式
│   ├── heron-git/                 # 远传输（裸仓缓存）
│   ├── heron-source/              # URL 解析 + catalog + 市场适配器
│   │   ├── src/
│   │   │   ├── lib.rs
│   │   │   ├── url.rs             # 四种形态 + GitLab 歧义探测（§6.1）
│   │   │   ├── catalog/           # marketplace.json / heron.source.toml / 兜底扫描
│   │   │   └── market/            # 适配器（§6.3）
│   │   │       ├── mod.rs         # MarketAdapter trait + registry
│   │   │       ├── skills_sh.rs
│   │   │       ├── clawhub.rs
│   │   │       ├── qoder.rs
│   │   │       ├── skillhub_cn.rs
│   │   │       └── skillsmp.rs
│   ├── heron-projection/          # 投影与可见性
│   │   ├── src/
│   │   │   ├── lib.rs
│   │   │   ├── plan.rs            # 生成投影计划（不落盘）
│   │   │   ├── apply.rs           # 执行计划（补偿式）
│   │   │   ├── verify.rs          # 可见性判定（只读识别客户端，§10.11）
│   │   │   └── nativescan.rs      # 原生扫描注入（能力表驱动，默认关）
│   │   ├── heron-service/         # 用例编排（GUI/CLI 共用）
│   │   └── heron-cli/             # clap CLI
├── ui/                            # Vue 3 前端（§8）
│   ├── package.json
│   ├── vite.config.ts
│   ├── index.html
│   └── src/
│       ├── main.ts / App.vue / router.ts / stores/ / api/ / i18n/ / views/ / components/
├── resources/
│   ├── clients/                   # 内置兼容性矩阵（打包进二进制）
│   │   ├── claude.toml
│   │   ├── agents.toml
│   │   ├── codex.toml
│   │   ├── cursor.toml
│   │   └── opencode.toml
│   └── templates/                 # heron new 的骨架
├── tests/
│   ├── fixtures/                  # 假仓库、病态嵌套、legacy-command、junction 样本
│   ├── e2e/                       # 端到端（跨 crate）
│   └── platform/                  # 平台差异用例（按 OS 跳过）
└── docs/
    ├── DESIGN-heron-v2.md
    ├── IMPLEMENTATION-heron-v1.md
    ├── legacy/DESIGN-v0.2.md
    └── adr/                       # 落地过程中新增的决策记录
```

### 1.2 crate 依赖图（编译期强制单向）

```text
heron-app (Tauri) ─┐
heron-cli        ──┼─▶ heron-service ─▶ heron-projection ─▶ heron-store
                   │                    ├─▶ heron-vcs ─▶ heron-git
                   │                    ├─▶ heron-source ─▶ heron-git
                   │                    └─▶ heron-scan
                   └──────────────────────▶ heron-core ◀── heron-platform
```

```toml
# crates/heron-core/Cargo.toml —— 仅此三项，且不含 IO/async
[dependencies]
serde = { version = "1", features = ["derive"] }
thiserror = "2"
semver = "1"
```

> 用 `cargo-deny` + 自写 `xtask check-deps`（读 `cargo metadata`）在 CI 中校验：`heron-core` 的依赖闭包中不得出现 `tokio` / `std::fs` 使用（以 `#![forbid(unsafe_code)]` + 依赖白名单双重保证）。

### 1.3 版本与依赖基线（2026-11-19 实测 crates.io）

| crate | 版本 | 用途 |
|-------|------|------|
| `tauri` | 2.12.x | GUI 壳 |
| `rusqlite` | 0.43（`bundled`） | 索引库 |
| `gix` | 0.90（只读，layers: no network） | 读取仓库状态 |
| `clap` | 4.6（derive） | CLI |
| `notify` | 8.2 | 文件监听 |
| `blake3` | 1.8 | 内容指纹 |
| `similar` | 3.2 | 文本 diff / 兜底合并 |
| `junction` | 2.1 | Windows 目录联接（§4.3） |
| `fs4` | 1.1 | 文件锁 |
| `directories` | 6.0 | 用户目录 |
| `dunce` | 1.0 | Windows 路径前缀往返 |
| `serde_yaml` | 0.9（deprecated）→ **改用 `saphyr` 或 `yaml-rust2`** | frontmatter 解析（§5.3 说明迁移策略） |
| 前端 | `vue@3` / `typescript@5` / `vite@7` / `pinia@3` / `vue-router@4` / `vue-i18n@11` / `tailwindcss@4` | 见 `ui/package.json` |

> `serde_yaml` 已进入 deprecated 状态，frontmatter 解析采用 `yaml-rust2`（宽松）+ 自写"标量提取"兜底（§5.3），不新增对 deprecated 库的依赖。

### 1.4 构建与 CI

```yaml
# .github/workflows/ci.yml（要点）
jobs:
  rust:
    strategy: { matrix: { os: [macos-14, windows-2022, ubuntu-22.04] } }
    steps:
      - cargo fmt --check
      - cargo clippy --all-targets -- -D warnings
      - cargo test --workspace                 # 平台无关用例
      - cargo test --workspace -- --ignored    # 平台相关（link/junction）
      - cargo deny check
      - xtask check-deps                       # §1.2 依赖规则
  ui:
    steps: [pnpm -C ui lint, pnpm -C ui typecheck, pnpm -C ui test:unit, pnpm -C ui i18n:check]
  e2e:
    needs: [rust, ui]
    steps:
      - pnpm tauri build --debug
      - pnpm test:e2e                          # 见 §10.4
```

**Windows CI 必须**：普通用户运行 `cargo test -- --ignored`（不做任何提权），以验证 junction 路径可创建、可删除、不误删目标。

**构建产物**：
```text
macOS   → heron_<ver>_aarch64.dmg / x86_64.dmg   （签名 + 公证）
Windows → heron_<ver>_x64-setup.exe (NSIS) / .msi （代码签名）
CLI     → heron-cli_<ver>_{os}_{arch}.tar.gz / .zip （随 GUI 附带，也可单独下载）
```

### 1.5 编码规范（摘要）

| 规则 | 说明 |
|------|------|
| 错误 | 库层返回 `Result<T, HeronError>`；`HeronError` 必含 `code: ErrorCode` 与 `params` |
| 日志 | `tracing`，字段优先（`skill = %slug`），不得把绝对路径写进 info 级（隐私，§15 R10） |
| 命名 | 集合用复数、动词用祈使式；领域名词严格对齐设计文档 §17 术语表 |
| 并发 | 扫描用 `rayon`（CPU 密集），网络用 `tokio`（Tauri 运行时）；**不允许**阻塞调用出现在 async 上下文 |
| 取消 | 所有长任务接收 `CancelToken`（`tokio_util::sync::CancellationToken`），每 N 步检查一次 |
| unsafe | 仅 `heron-platform/windows.rs` 允许，且必须 `// SAFETY:` 注释 + 单测覆盖 |

---

## 2. 数据库

### 2.1 连接与 PRAGMA

```rust
// crates/heron-store/src/db.rs
pub struct Db { pool: r2d2::Pool<SqliteConnectionManager> }

impl Db {
    pub fn open(path: &Path) -> Result<Self, HeronError> {
        let mgr = SqliteConnectionManager::file(path)
            .with_init(|c| {
                c.busy_timeout(Duration::from_secs(5))?;
                c.pragma_update(None, "journal_mode", "WAL")?;
                c.pragma_update(None, "synchronous", "NORMAL")?;
                c.pragma_update(None, "foreign_keys", "ON")?;
                c.pragma_update(None, "temp_store", "MEMORY")?;
                Ok(())
            });
        let pool = r2d2::Pool::builder().max_size(4).build(mgr)?;
        migrate(&mut pool.get()?)?;      // §2.2
        Ok(Self { pool })
    }
}
```

* **单写多读**：写事务通过 `Db::write(|tx| …)` 串行获取；
* 迁移在**获取进程级锁之后**执行（`platform::lock::single_flight("db-migrate")`），避免 CLI 与 GUI 并发迁移；
* 索引损坏（`PRAGMA quick_check` 失败）→ 重命名为 `heron.db.corrupt.<ts>` 并全量重建（设计 §10.9）。

### 2.2 迁移机制

```rust
// 文件名即版本号：0001_init.sql, 0002_add_native_scan.sql …
const MIGRATIONS: &[(&str, &str)] = &[
    ("0001_init", include_str!("migrations/0001_init.sql")),
];

fn migrate(conn: &mut Connection) -> Result<(), HeronError> {
    conn.pragma_update(None, "user_version", 0)?;              // 仅首次
    let cur: i64 = conn.pragma_query_value(None, "user_version", |r| r.get(0))?;
    for (i, (name, sql)) in MIGRATIONS.iter().enumerate() {
        let ver = (i + 1) as i64;
        if ver <= cur { continue; }
        let tx = conn.transaction()?;
        tx.execute_batch(sql)?;
        tx.pragma_update(None, "user_version", ver)?;
        tx.commit().map_err(|e| HeronError::db(name, e))?;
    }
    Ok(())
}
```

规则：**迁移只增不改**；破坏性变更（删列）必须先发布一个"双写"版本的迁移，再在下一版本删列。

### 2.3 `0001_init.sql`（完整 DDL）

```sql
-- 与设计文档 §7.3 一致；此处为可直接执行的最终形态
CREATE TABLE client (
  id            TEXT PRIMARY KEY,
  display_name  TEXT NOT NULL,
  global_paths  TEXT NOT NULL,
  project_paths TEXT NOT NULL,
  link_modes    TEXT NOT NULL,
  priority      INTEGER NOT NULL DEFAULT 0,
  native_scan   TEXT,                          -- JSON: {supported, method, config_path, evidence}
  verified_at   TEXT,
  verified_by   TEXT
);

CREATE TABLE skill (
  slug         TEXT PRIMARY KEY,
  display_name TEXT NOT NULL,
  description  TEXT NOT NULL DEFAULT '',
  tags         TEXT NOT NULL DEFAULT '[]',
  status       TEXT NOT NULL DEFAULT 'active' CHECK (status IN ('active','trashed')),
  origin       TEXT NOT NULL DEFAULT 'store'
               CHECK (origin IN ('store','adopted','legacy-command')),
  created_at   TEXT NOT NULL,
  updated_at   TEXT NOT NULL
);

CREATE TABLE source_binding (
  id             INTEGER PRIMARY KEY,
  slug           TEXT NOT NULL REFERENCES skill(slug) ON DELETE CASCADE,
  kind           TEXT NOT NULL CHECK (kind IN ('git','local','adopted')),
  url            TEXT,
  ref            TEXT,
  subpath        TEXT,
  pinned_commit  TEXT,
  remote_version TEXT,
  content_hash   TEXT,
  is_primary     INTEGER NOT NULL DEFAULT 1,
  UNIQUE(slug, url, subpath)
);
CREATE INDEX idx_binding_slug ON source_binding(slug);

CREATE TABLE version (
  slug         TEXT NOT NULL REFERENCES skill(slug) ON DELETE CASCADE,
  version      TEXT NOT NULL,
  commit_sha   TEXT NOT NULL,
  content_hash TEXT NOT NULL,
  size_bytes   INTEGER NOT NULL DEFAULT 0,
  fetched_at   TEXT NOT NULL,
  origin       TEXT NOT NULL CHECK (origin IN ('upstream','local','merge')),
  PRIMARY KEY (slug, version)
);

CREATE TABLE target (
  id           TEXT PRIMARY KEY,               -- 'heron:global' / 'claude:project:<hash>'
  kind         TEXT NOT NULL REFERENCES client(id),
  scope        TEXT NOT NULL CHECK (scope IN ('global','project')),
  project_path TEXT,
  path         TEXT NOT NULL,
  enabled      INTEGER NOT NULL DEFAULT 1,
  is_default   INTEGER NOT NULL DEFAULT 0,
  discover     TEXT NOT NULL DEFAULT 'manual' CHECK (discover IN ('auto','manual')),
  UNIQUE(kind, scope, path)
);

CREATE TABLE projection (
  id           INTEGER PRIMARY KEY,
  slug         TEXT NOT NULL REFERENCES skill(slug),
  target_id    TEXT NOT NULL REFERENCES target(id) ON DELETE CASCADE,
  link_name    TEXT NOT NULL,
  path         TEXT NOT NULL,
  mode         TEXT NOT NULL CHECK (mode IN ('copy','symlink','junction','pointer')),
  version      TEXT,
  content_hash TEXT,
  inode_kind   TEXT,                           -- 落地时的真实类型：reparse_symlink/reparse_junction/dir/file
  state        TEXT NOT NULL DEFAULT 'ok'
               CHECK (state IN ('ok','broken','drifted','modified','shadowed')),
  created_at   TEXT NOT NULL,
  UNIQUE(target_id, link_name)
);
CREATE INDEX idx_projection_slug ON projection(slug);

CREATE TABLE source (
  id             TEXT PRIMARY KEY,
  name           TEXT NOT NULL,
  kind           TEXT NOT NULL CHECK (kind IN ('git','local','market')),
  adapter        TEXT,
  url            TEXT,
  ref            TEXT,
  subpath        TEXT,
  path           TEXT,
  priority       INTEGER NOT NULL DEFAULT 100,
  trusted        INTEGER NOT NULL DEFAULT 0,
  enabled        INTEGER NOT NULL DEFAULT 1,
  capability     TEXT,                          -- market: 'indexed' | 'guided'
  last_synced_at TEXT,
  last_commit    TEXT,
  last_probe_ok  TEXT
);

CREATE TABLE detected (
  path           TEXT PRIMARY KEY,
  real_path      TEXT,
  client_id      TEXT,
  scope          TEXT,
  project_path   TEXT,
  observed_slug  TEXT,
  content_hash   TEXT,
  frontmatter_ok INTEGER,
  kind           TEXT NOT NULL DEFAULT 'skill'
                 CHECK (kind IN ('skill','legacy-command','nested-suspect')),
  last_seen_at   TEXT NOT NULL,
  adopt_state    TEXT NOT NULL DEFAULT 'candidate'
                 CHECK (adopt_state IN ('candidate','ignored','adopted','conflict'))
);
CREATE INDEX idx_detected_state ON detected(adopt_state);

CREATE TABLE visibility (
  projection_id INTEGER PRIMARY KEY REFERENCES projection(id) ON DELETE CASCADE,
  client_id     TEXT NOT NULL,
  state         TEXT NOT NULL
                CHECK (state IN ('visible','not_scanned','shadowed','restart_required','broken')),
  shadowed_by   TEXT,
  checked_at    TEXT NOT NULL
);

CREATE TABLE event (
  id        INTEGER PRIMARY KEY,
  ts        TEXT NOT NULL,
  actor     TEXT NOT NULL CHECK (actor IN ('gui','cli','system')),
  op        TEXT NOT NULL,
  subject   TEXT,
  detail    TEXT,
  undo_of   INTEGER REFERENCES event(id),
  undoable  INTEGER NOT NULL DEFAULT 0
);
CREATE INDEX idx_event_ts ON event(ts DESC);

CREATE TABLE undo_step (          -- 补偿日志：支撑"撤销上次操作"（§4.7）
  event_id  INTEGER NOT NULL REFERENCES event(id) ON DELETE CASCADE,
  seq       INTEGER NOT NULL,
  action    TEXT NOT NULL,        -- JSON: {kind:'created_link', path:'…'} 等
  PRIMARY KEY (event_id, seq)
);

CREATE TABLE kv (                 -- 索引版本、上次扫描、schema 版本等
  k TEXT PRIMARY KEY,
  v TEXT NOT NULL
);
```

### 2.4 关键查询（实现时按此写，避免 N+1）

```sql
-- 首页统计（一条语句）
SELECT
  (SELECT COUNT(*) FROM skill WHERE status='active')                    AS skills,
  (SELECT COUNT(*) FROM detected WHERE adopt_state='candidate')         AS unmanaged,
  (SELECT COUNT(*) FROM projection WHERE state<>'ok')                   AS broken,
  (SELECT COUNT(*) FROM visibility WHERE state='shadowed')              AS shadowed;

-- 某 Skill 的全部落点 + 可见性
SELECT p.target_id, t.kind, t.scope, t.project_path, p.mode, p.state,
       COALESCE(v.state,'unknown') AS visibility
FROM projection p JOIN target t ON t.id = p.target_id
LEFT JOIN visibility v ON v.projection_id = p.id
WHERE p.slug = ?1;

-- 可升级：基线 commit 与来源最新 commit 不同且未被 pin
SELECT s.slug, b.pinned_commit, so.last_commit
FROM skill s JOIN source_binding b ON b.slug = s.slug
JOIN source so ON so.url = b.url
WHERE so.enabled = 1 AND b.pinned_commit <> so.last_commit;
```

---

## 3. 领域接口（heron-core）

> 本节给出**可直接照着写实现**的签名。字段命名与设计文档 §7.3 一一对应。

### 3.1 核心类型

```rust
// slug.rs
#[derive(Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Slug(String);

pub enum SlugPolicy { Strict, Lenient }   // Strict：校验平台合法性

impl Slug {
    /// 从 frontmatter name / 目录名 / legacy-command 文件名推导
    pub fn derive(raw: &str, policy: SlugPolicy) -> Result<Self, Msg>;
    pub fn as_str(&self) -> &str;
    /// 大小写不敏感比较（Windows 语义，全平台统一）
    pub fn eq_ci(&self, other: &Slug) -> bool;
    /// 平台合法性：Windows 保留名/尾随点空格/非法字符；长度 <= 64
    pub fn validate_platform(&self) -> Result<(), Msg>;
}

// version.rs
#[derive(Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum Version {
    Semver(semver::Version),
    Fallback { base: semver::Version, sha: String },  // 0.0.0+<short sha>
    Opaque(String),                                    // metadata.version 非 semver（如 "42"）
}
impl Version {
    pub fn parse(raw: &str, sha_hint: Option<&str>) -> Self;
    pub fn is_upgrade_from(&self, other: &Version, lane: UpgradeLane) -> bool;
}
pub enum UpgradeLane { Exact, SameMajor, Any }

// policy.rs
pub enum PinPolicy { Exact, Semver, None }
pub enum ConflictPolicy { Ask, KeepOld, Overwrite, SideBySide }
pub enum ProjectionMode { Copy, Symlink, Junction, Pointer }
impl ProjectionMode { pub fn resolve_auto(os: Platform) -> Self; }   // macOS/Linux→Symlink, Windows→Junction

// ids.rs
pub struct TargetId(String);   // "<kind>:global" | "<kind>:project:<sha256(project_path)[..8]>"
pub struct SourceId(String);
pub struct MarketId(String);   // skills_sh | clawhub | qoder | skillhub_cn | skillsmp
```

### 3.2 消息与错误

```rust
// error.rs —— 永不内联自然语言
#[derive(Serialize, Clone, Debug)]
pub struct Msg {
    pub code: &'static str,                    // 'E_GIT_MISSING' / 'E_MERGE_CONFLICT' …
    pub params: std::collections::BTreeMap<String, String>,
    pub severity: Severity,                    // Info | Warn | Error
}
#[derive(Debug, thiserror::Error)]
pub enum HeronError {
    #[error("{}", .0.code)]
    Msg(Msg),
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    Db(#[from] rusqlite::Error),
}
pub type Result<T, E = HeronError> = std::result::Result<T, E>;
```

错误码全集见 §10.6；**新增错误码必须同时提交中英文案（§9.2 CI 校验）**。

### 3.3 前后端共享契约

```rust
// crates/heron-core/src/dto.rs —— 序列化后即 TS 类型（通过 ts-rs 或手写 ui/src/api/types.ts）
#[derive(Serialize)] pub struct SkillSummary {
    pub slug: String, pub name: String, pub description: String,
    pub version: Option<String>, pub status: SkillStatus,
    pub origin: Origin,                 // store | adopted | legacy-command
    pub source: Option<SourceRef>,      // url + ref + subpath（脱敏：仅保留 host 与 path）
    pub projections: Vec<ProjectionView>,
    pub flags: SkillFlags,              // has_local_changes / upgradable / legacy / unmanaged
}
#[derive(Serialize)] pub struct ProjectionView {
    pub target: TargetRef, pub mode: ProjectionMode,
    pub visibility: Visibility, pub state: ProjectionState,
}
```

> 约定：**所有 Tauri command 的入参与返回都使用本模块的 DTO**，不直接暴露数据库行或 `PathBuf`（路径在 Rust 侧转为字符串并统一用 `/` 分隔，前端不拼路径）。

---

## 4. 平台层实现（heron-platform）

### 4.1 目录解析

```rust
pub struct Dirs { pub data: PathBuf, pub config: PathBuf, pub cache: PathBuf, pub log: PathBuf }
pub fn resolve(app: &str) -> Dirs;   // directories::ProjectDirs::from("io","greyheron",app)

// 影子数据目录：允许用户把数据目录搬到外置盘，其它三项仍留在系统默认位置
pub fn resolve_with_override(override_data: Option<&Path>) -> Result<Dirs, Msg>;

| 平台 | data | config | cache | log |
|------|------|--------|-------|-----|
| macOS | `~/Library/Application Support/heron` | 同 data | `~/Library/Caches/heron` | `~/Library/Logs/heron` |
| Windows | `%LOCALAPPDATA%\\heron` | `%APPDATA%\\heron` | `%LOCALAPPDATA%\\heron\\cache` | data\\log |
| Linux | `~/.local/share/heron` | `~/.config/heron` | `~/.cache/heron` | data/log |
```

> 决策理由见设计 ADR-8：Windows 用 `LOCALAPPDATA` 而非 roaming，避免 OneDrive 漫游把链接变成占位符。

### 4.2 路径工具

```rust
pub fn extended(path: &Path) -> Cow<Path>;     // Windows: 加 \\\\?\\ 前缀；其它平台原样
pub fn display(path: &Path) -> String;         // 反向：去前缀 + 正斜杠 + 家目录缩写为 ~
pub fn normalize_case(path: &Path) -> PathBuf; // 仅用于比较（Windows 小写化）
pub fn is_same_or_inside(parent: &Path, child: &Path) -> bool;
pub fn ensure_dir_within(root: &Path, target: &Path) -> Result<(), Msg>;  // 防止越界写入
```

**规则**：所有 FS 调用必须通过本模块返回的 `Path`，禁止在其它 crate 直接使用 `std::fs` 处理可能超长的路径。

### 4.3 链接模块（风险 R3 的唯一出口）

```rust
// link/mod.rs
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LinkKind { SymbolicDir, Junction, File, Missing }
pub struct LinkInfo {
    pub kind: LinkKind,
    /// 指向的目标（原始字符串，不一定存在）
    pub target: Option<PathBuf>,
    /// 目标是否存在且可读
    pub resolved_exists: bool,
}

pub fn probe(path: &Path) -> Result<LinkInfo, Msg>;          // 不跟随链接
pub fn create_dir_link(target: &Path, link: &Path, mode: ProjectionMode) -> Result<LinkKind, Msg>;
pub fn remove_link(path: &Path) -> Result<(), Msg>;           // **先解除链接，绝不递归删目标内容**
pub fn relink(path: &Path, new_target: &Path) -> Result<(), Msg>; // remove_link + create
```

**Windows 实现要点（platform/src/link/windows.rs）**

| 动作 | 实现 |
|------|------|
| `probe` | `GetFileAttributesW` 取 `FILE_ATTRIBUTE_REPARSE_POINT`；再用 `FindFirstFileW` 的 `WIN32_FIND_DATAW.dwReserved0` 区分 `IO_REPARSE_TAG_MOUNT_POINT (0xA0000003)` 与 `IO_REPARSE_TAG_SYMLINK (0xA000000C)`；目标字符串用 `FSCTL_GET_REPARSE_POINT` 读取 |
| `create_dir_link(Junction)` | `junction::create(target, link)`（crate 2.1）；前置断言：`target` 必须是已存在目录、`link` 必须不存在 |
| `create_dir_link(SymbolicDir)` | `std::os::windows::fs::symlink_dir`；失败 `ERROR_PRIVILEGE_NOT_HELD` → 按配置**自动降级为 junction** 并写 `WARN E_LINK_PRIVILEGE` |
| `remove_link` | **必须**：`probe` 确认 `kind` 是链接 → 用 `RemoveDirectoryW`（对 junction 只移除 reparse point，不进入目标）；**禁止** `std::fs::remove_dir_all`。对 **file symlink** 用 `DeleteFileW` |
| `resolved_exists` | `probe` 后对 `target` 做 `metadata`；失败即 `broken` |

```rust
// 必须的反向断言：删除后确认目标内容仍在
fn remove_link_guarded(path: &Path) -> Result<(), Msg> {
    let info = probe(path)?;
    if info.kind == LinkKind::Missing { return Ok(()); }
    if info.kind == LinkKind::File { /* 实体目录：拒绝删除 */ return Err(SafetyDeleteRefused(path.into())); }
    let target = info.target.clone();
    remove_link_raw(path)?;                       // RemoveDirectoryW / unlink
    if let Some(t) = target { assert!(t.exists() || info.resolved_exists == false); }
    Ok(())
}
```

**macOS / Linux（link/unix.rs）**：`symlink` / `read_link` / `remove_file`；`probe` 用 `symlink_metadata`。

**幂等性**：`create_dir_link` 若 `link` 已存在且 `probe` 指向同一 `target` → 返回 `Ok` 并标记 `Unchanged`（映射为 `ProjectionOutcome::AlreadyPresent`）。

### 4.4 原子写与文件锁

```rust
pub fn write_atomic(path: &Path, bytes: &[u8]) -> Result<(), Msg>;   // tmp(同目录) → fsync → rename
pub fn read_to_string_lenient(path: &Path) -> Result<String, Msg>;   // 去 BOM、CRLF→LF

pub struct FileLock(/* OS handle */);
pub fn single_instance(name: &str) -> Result<Option<FileLock>, Msg>;  // None = 已有实例
pub fn exclusive(name: &str) -> Result<FileLock, Msg>;                // 阻塞获取，带超时
```

### 4.5 子进程封装（git）

```rust
pub struct Cmd<'a> { program: &'a str, args: Vec<String>, env: Vec<(String,String)>, timeout: Duration }
pub struct CmdOutput { status: Option<i32>, stdout: String, stderr: String }

pub async fn run_async(cmd: Cmd<'_>, cancel: &CancelToken) -> Result<CmdOutput, Msg>;
// 规则：stdin=null、stdout/stderr 捕获上限 1 MiB（超出丢弃并标记 truncated）、
//      超时/取消 → kill 进程树（Windows 用 Job Object，Unix 用进程组），返回 E_NETWORK/E_CANCELLED
```

**git 环境隔离**：所有 git 调用统一加 `GIT_TERMINAL_PROMPT=0`、`GIT_ASKPASS=`、`LC_ALL=C`（保证 stderr 可解析且不阻塞交互），凭据交由系统 credential helper。

### 4.6 探测

```rust
pub struct Preflight { pub git: Option<GitInfo>, pub os: Platform, pub webview: WebviewInfo }
pub fn preflight() -> Preflight;
pub struct GitInfo { pub version: String, pub path: PathBuf, pub supports_merge_tree: bool }
```

`git --version` 解析 `(\d+)\.(\d+)`，`supports_merge_tree = version >= (2, 38)`；低于该版本时三方合并走 `similar` 兜底（§5.5）。

### 4.7 事务模型（补偿式）

```rust
pub struct Operation { event_id: i64, steps: Vec<UndoAction>, done: Vec<UndoAction> }

impl Operation {
    pub fn new(db: &Db, op: &str, subject: Option<&str>) -> Result<Self, Msg>;   // 写 event(undoable=1)
    pub fn step<F, U>(&mut self, do_it: F, undo: U) -> Result<(), Msg>
        where F: FnOnce() -> Result<(), Msg>, U: FnOnce() -> Result<(), Msg>;
    pub fn commit(self) -> Result<i64, Msg>;      // 清空 undo_step（不可再撤销）
    pub fn rollback(self) -> Result<(), Msg>;     // 逆序执行已完成的 undo
}
```

**规则**：`adopt` / `install` / `project` / `upgrade` / `rollback` 必须使用 `Operation`；进程崩溃后 `heron doctor --repair` 扫描 `event.undoable=1` 的残留事件并提示恢复。

---

## 5. 扫描、存储与版本（heron-scan / heron-store / heron-vcs / heron-git）

### 5.1 扫描器（heron-scan）

```rust
pub struct ScanOptions {
    pub roots: Vec<PathBuf>,            // 已解析的默认根 + 用户额外根
    pub max_depth: usize,               // 默认 8
    pub max_dirs: usize,                // 默认 50000
    pub follow_links: bool,             // 默认 false
    pub exclude: Vec<String>,           // globset 编译
    pub cancel: CancelToken,
}
pub struct ScanResult {
    pub hits: Vec<Hit>,                 // 命中项
    pub stats: ScanStats,               // 目录数、耗时、跳过原因计数
    pub nested: Vec<NestedSuspect>,     // 病态嵌套（D8：只报告）
}
pub struct Hit {
    pub path: PathBuf,
    pub real_path: PathBuf,             // 若为链接，解析后的目标
    pub kind: HitKind,                  // Skill | LegacyCommand | NestedSuspect
    pub client: Option<ClientId>, pub scope: Scope, pub project: Option<PathBuf>,
    pub frontmatter: Option<Frontmatter>,
    pub content_hash: Option<Hash>,     // 惰性：仅当需要去重/比较时计算
}

impl Scanner {
    pub fn scan(&self, opt: &ScanOptions) -> Result<ScanResult, Msg>;   // 同步 + rayon
}
```

**遍历规则（必须逐条实现并有单测）**：

| 规则 | 说明 |
|------|------|
| 边界 | 命中"含 `SKILL.md` 的目录"即停止向下递归；`.claude/commands` 下命中 `*.md` 即记录且不递归 |
| 忽略 | `node_modules`、`.git`、`target`、`dist`、`.venv`、`.codex/vendor_imports`、`.vscode/extensions`、`store/**/.git/**`、`.DS_Store` |
| 防环 | `follow_links=false` 时一律不跟随；即便开启，也用 `(dev, ino)` 集合去重（Windows 用 `file_index`） |
| 上限 | `max_dirs` 触达 → 停止并返回 `E_SCAN_LIMIT`（Warn，非失败） |
| 性能 | 只读 `SKILL.md` 的前 64 KiB；条目元数据（`mtime`、`size`）与索引比对，未变化则**不重新解析** |
| 病态嵌套 | 命中路径中包含 `/.claude/skills/.claude/` 或 `/.agents/skills/.agents/` → 额外产出 `NestedSuspect`，**不移动不删除**（D8） |

### 5.2 默认扫描根（数据驱动，来自 `resources/clients/*.toml`）

```toml
# resources/clients/claude.toml（节选）
id = "claude"
display_name = "Claude Code"
global_paths = ["~/.claude/skills"]
project_paths = [".claude/skills"]
link_modes = ["symlink", "junction", "copy"]
priority = 20
verified_at = "2026-11-19"
verified_by = "doc"          # spec | doc | probe

[native_scan]
supported = false            # 未验证前一律 false（设计 §10.11 原则 5）
evidence = "no documented extra-skill-dir setting as of 2026-11-19"
```

首次启动把 `resources/clients/*.toml` 导入 `client` 表；用户可在 `<DATA>/clients/overrides.toml` 覆盖（合并策略：字段级覆盖，`verified_by=probe` 优先）。

### 5.3 frontmatter 解析（两级）

```rust
pub struct Frontmatter {
    pub name: Option<String>,
    pub description: Option<String>,
    pub license: Option<String>,
    pub compatibility: Option<String>,
    pub metadata: BTreeMap<String, String>,
    pub allowed_tools: Option<String>,
    pub raw_yaml: String,
    pub confidence: Confidence,     // Full | Partial | None
}
pub fn parse_skill_md(bytes: &[u8]) -> (Frontmatter, BodyRange);
```

| 级别 | 触发 | 行为 |
|------|------|------|
| 一级 | 标准 YAML（`yaml-rust2`） | 完整解析 |
| 二级 | 一级失败 | "顶层标量提取"：逐行匹配 `key: value`，支持 `|` / `>` 块标量与引号折叠（与用户现有 `sync_skills.py` 的策略一致） |
| 失败 | 无 frontmatter | `confidence = None`；仍可被收拢，但体检报 `E_FRONTMATTER` |

**校验规则**（设计 §1.3 与 Agent Skills 规范）：`name` ≤ 64、小写字母/数字/连字符、不得首尾连字符；`description` 非空且 ≤ 1024；超限在 `doctor` 报 Warn，**不阻断安装**。

### 5.4 指纹（规范化哈希）

```rust
pub fn fingerprint_dir(root: &Path, opts: &FingerprintOptions) -> Result<Hash, Msg>;
// 规则（设计 §10.3）：忽略 .git/.DS_Store/Thumbs.db/.heron；
// 路径转正斜杠并按字节序排序；文本文件 CRLF→LF；权限位不参与；
// 符号链接以 "L:<target 字符串>" 形式计入内容
```

**快捷判定**：目录 `mtime` 与 `size` 总和未变 → 复用索引中的哈希（避免每次启动全量哈希）。

### 5.5 每 Skill 的 Git 仓库（heron-vcs）

```text
store/<slug>/
├── .git/                    # 独立仓库
├── heron.skill.json         # 本地元数据
├── SKILL.md
└── …
```

`heron.skill.json`（**纳入版本控制**，随 Skill 一起被投影）：

```json
{
  "schema": 1,
  "slug": "find-skills",
  "origin": "upstream",
  "source": { "url": "https://github.com/anthropics/skills.git", "ref": "main", "subpath": "skills/find-skills" },
  "baseline": { "commit": "5f2d1c0…", "treeHash": "blake3:…", "version": "1.4.0" },
  "localRevision": 3,
  "createdAt": "2026-11-19T08:00:00Z"
}
```

**分支约定**：

| 分支/标签 | 语义 |
|-----------|------|
| `main` | 工作分支 = 当前生效内容（本地改动也提交在这里） |
| `upstream` | 远端镜像的最近一次落地提交（只由 Heron 写入） |
| `tag: v<version>` | 每次成功落地上游版本的标记（回滚目标） |
| `tag: heron-merge-<ts>` | 每次成功三方合并的标记（合并审计） |

**接口**：

```rust
pub struct SkillRepo { root: PathBuf, git: GitCli }
impl SkillRepo {
    pub fn init(root: &Path, meta: &SkillMeta) -> Result<Self, Msg>;
    pub fn commit_all(&self, msg: &str) -> Result<CommitSha, Msg>;
    pub fn set_upstream_point(&self, commit: &str) -> Result<(), Msg>;
    pub fn tag_version(&self, version: &str, commit: &str) -> Result<(), Msg>;
    pub fn log(&self, limit: usize) -> Result<Vec<CommitInfo>, Msg>;   // gix 只读
    pub fn changed_since_baseline(&self) -> Result<DiffSummary, Msg>;
    pub fn merge_from_upstream(&self, new_commit: &str, policy: MergePolicy)
        -> Result<MergeOutcome, Msg>;
    pub fn reset_to(&self, rev: &str) -> Result<(), Msg>;              // 先打 rollback-backup 标签
}
```

**三方合并策略（`merge_from_upstream`）**：

1. 若工作区有未提交改动 → 先 `commit_all("heron: pre-upgrade snapshot")`；
2. `git fetch` 远端（经 `heron-git` 的 bare 缓存，见 §5.6）到临时引用；
3. `git merge-tree --write-tree base ours theirs`（git ≥ 2.38）：
   * 退出码 0 → 得到合并树，`git commit-tree` + `read-tree` 落地；
   * 退出码非 0 → 解析冲突路径清单，返回 `MergeOutcome::Conflicts(Vec<ConflictFile>)`；
4. 低版本 git（< 2.38）→ 逐文件用 `similar::TextMerge` 做三方合并，二进制文件直接判冲突；
5. 合并后**校验**：`SKILL.md` 可解析、`fingerprint_dir` 成功、不出现 `.orig`/`.rej` 残留；
6. 成功 → 更新 `heron.skill.json.baseline`、打 tag、返回 `MergeOutcome::Clean{ new_tree, version }`；任何一步失败 → `git reset --hard <pre-upgrade sha>` 并回滚基线的 JSON 修改。

```rust
pub enum MergeOutcome {
    Clean { new_commit: String, version: Version, files_changed: usize },
    Conflicts { files: Vec<ConflictFile> },   // GUI 三栏合并（§8.5）
}
pub struct ConflictFile { pub path: String, pub base: String, pub ours: String, pub theirs: String }
```

### 5.6 裸仓缓存（heron-git）

```text
<DATA>/sources-cache/<sha256(url)>/bare.git/     # git clone --bare / fetch --prune
```

```rust
pub struct RemoteRepo { bare: PathBuf, url: String }
impl RemoteRepo {
    pub async fn ensure(&mut self, cancel: &CancelToken) -> Result<(), Msg>;   // 存在则 --depth=1 fetch
    pub fn tree_at(&self, rev: &str, subpath: &str) -> Result<Vec<TreeEntry>, Msg>;  // gix 只读
    pub fn resolve_ref(&self, r: &str) -> Result<CommitSha, Msg>;
    pub async fn sparse_checkout(&self, rev: &str, paths: &[&str], dest: &Path, cancel: &CancelToken)
        -> Result<(), Msg>;   // git archive | tar, 或 sparse-checkout + worktree
}
```

规则：缓存目录损坏 → 删除重建（可安全删除，设计 §7.1）；同一 URL 的并发 `ensure` 用 `exclusive("git:<hash(url)>")` 串行化。

---

## 6. 来源与市场适配器（heron-source）

### 6.1 URL 解析（`url.rs`）

```rust
pub enum SourceSpec {
    Git { url: String, ref_: Option<String>, subpath: Option<String> },
    Local(PathBuf),
    MarketItem { market: MarketId, key: String },
    Archive(String),          // Phase 3
}
pub fn parse(input: &str, hint: Option<VcsHint>) -> Result<Parsed, Msg>;
pub struct Parsed { pub spec: SourceSpec, pub ambiguous: Option<Vec<Candidate>>, pub notes: Vec<Msg> }
```

| 输入 | 处理 |
|------|------|
| `owner/repo` | 恰两段、无冒号 → github 简写 |
| `https://github.com/o/r/tree/<ref>/<path>` | 切出 ref 与 subpath；ref 含 `/` 时按"最长可解析 ref"试探（先 `info/refs` 探测） |
| `https://gitlab.com/**` | 用 `git ls-remote` 逐段收缩试探仓库边界；无法唯一确定 → 返回 `ambiguous`（GUI 展示候选，CLI 要求 `--vcs` 或显式子目录） |
| `git@host:...` / `ssh://` | 直接作为 git URL |
| 本地路径 | 存在则 Local；含 `SKILL.md` 视为单 Skill |

探测使用 `git ls-remote --heads <candidate>`，超时 8 s，结果缓存 10 分钟（键：候选 URL）。

### 6.2 catalog 解析优先级

```rust
pub enum CatalogSource { MarketplaceJson, HeronToml, ScanFallback }
pub async fn load_catalog(repo: &RemoteRepo, rev: &str, cancel: &CancelToken)
    -> Result<(CatalogSource, Vec<CatalogItem>), Msg>;
```

* `.claude-plugin/marketplace.json`：按设计 §9.5.1 映射；`source` 为相对路径 → 扫描其 `skills/*`；为对象（`github`/`url`/`git-subdir`）→ 标记 `External` 并在 UI 标注"需二次拉取"；
* 两种 `git-subdir` 写法（`{source:"git-subdir",url,path}` 与 `{source:"git",url,path}`）**都要接受**，统一归一化；
* `heron.source.toml`：直接映射；
* 兜底扫描：`max_depth = 6`，忽略 `.git`、`node_modules`、`tests/fixtures`，命中即边界。

### 6.3 `MarketAdapter`（设计 §9.5）

```rust
#[async_trait]
pub trait MarketAdapter: Send + Sync {
    fn id(&self) -> MarketId;
    fn capability(&self) -> Capability;                        // Indexed | Guided
    async fn search(&self, q: &Query, cancel: &CancelToken) -> Result<Page<MarketItem>, Msg>;
    async fn get(&self, key: &MarketKey, cancel: &CancelToken) -> Result<MarketItem, Msg>;
    async fn resolve_install(&self, item: &MarketItem) -> Result<InstallPlan, Msg>;
    async fn probe(&self, cancel: &CancelToken) -> Result<Health, Msg>;
}
pub enum InstallPlan { Git { url: String, ref_: Option<String>, subpath: String }, Guided { open_url: String, hint: Msg } }
```

**共用设施**：`HttpClient`（reqwest，`User-Agent: heron/<ver>`，超时 15 s，重试 2 次指数退避，**自动继承系统代理**）；响应缓存 `<DATA>/sources-cache/market/<id>/<hash>.json`（TTL 10 分钟，离线可用并标 `stale:true`）。

#### 6.3.1 skills.sh（Indexed）

| 能力 | 实现 |
|------|------|
| 目录发现 | 拉 `https://www.skills.sh/sitemap.xml` → 子 sitemap（`sitemap-owners.xml`、`sitemap-skills-1..n.xml`）；URL 形如 `/<owner>/<repo>/<skill>` |
| 条目 | `MarketItem{ slug, owner, repo, skill, url }`；`install_plan = Git{ url: "https://github.com/<owner>/<repo>.git", subpath: "skills/<skill>" }` |
| 搜索 | 站点无 JSON API → 本地对已缓存 sitemap 条目做子串匹配（首次拉取后本地索引，约数万条，用 `tinyvec`/前缀索引加速） |
| 详情补充 | 可选：抓详情页 HTML，提取 `github.com/<owner>/<repo>` 与描述（失败不影响主流程） |
| 失败降级 | sitemap 不可用 → 转为 Guided（`https://www.skills.sh/search?q=<kw>`） |

#### 6.3.2 clawhub.ai（Indexed，条件）

| 接口 | 用途 |
|------|------|
| `GET /api/v1/packages?limit=50&cursor=…` | 列表（`family: skill` 过滤掉 plugin） |
| `GET /api/v1/packages/<name>` | 详情（`latestVersion`、`ownerHandle`） |
| `GET /api/v1/packages/<name>/versions` | 版本与 changelog |
| `GET /api/v1/skills?limit=…` | Skill 专用列表 |
| `GET /api/v1/search?q=…` | 搜索（返回 `install.reference` 与 `links.source`） |

**安装解析**：`links.source`（GitHub URL）存在 → `Git`；为空 → `Guided{ open_url: https://clawhub.ai/<owner>/skills/<slug> }` 并给 `E_MARKET_NO_GIT_SOURCE` 提示（设计 §9.5 A 档"有条件"）。

#### 6.3.3 qoder / skillhub_cn / skillsmp（Guided）

```rust
impl MarketAdapter for QoderAdapter {
    fn capability(&self) -> Capability { Capability::Guided }
    async fn resolve_install(&self, item: &MarketItem) -> Result<InstallPlan, Msg> {
        Ok(InstallPlan::Guided { open_url: item.url.clone(), hint: Msg::new("H_MARKET_GUIDED") })
    }
}
```

* 搜索：直接在系统浏览器打开带关键词的深链（`qoder.com/zh/marketplace?q=…`、`skillhub.cn/skills?q=…`）；
* Heron 侧提供"粘贴地址回投"输入框，进入 §6.1 的解析 → §5 安装流程；
* **不抓取**这些站点的页面（robots/反爬约束，设计 §9.5 实测表）。

### 6.4 来源注册表的启动初始化

首次启动（`kv` 无 `sources_initialized`）时写入 5 条内置来源（设计 §7.4）。之后**只做增量**：若用户删除了某内置来源，不重新写入（记录在 `kv:bootstrap_skipped`）。

---

## 7. 投影、服务层与命令面

### 7.1 投影计划与执行（heron-projection）

```rust
pub enum ProjectStep {
    CreateDir { path: PathBuf },
    CopyTree { from: PathBuf, to: PathBuf, hash: Hash },
    MakeLink { target: PathBuf, link: PathBuf, mode: ProjectionMode },
    WritePointer { path: PathBuf, real: PathBuf },          // pointer 模式：SKILL.md 指针文件
    RemoveLink { path: PathBuf },
    RemoveTree { path: PathBuf },                            // 仅 copy 模式且哈希一致时允许
    Backup { from: PathBuf, to: PathBuf },                   // 覆盖前的安全备份
}
pub struct ProjectPlan { pub steps: Vec<ProjectStep>, pub warnings: Vec<Msg>, pub token_estimate: u32 }

pub fn plan_project(db: &Db, slug: &Slug, target: &TargetId, mode: ProjectionMode)
    -> Result<ProjectPlan, Msg>;     // 纯计算，不落盘；GUI 预览与 CLI --dry-run 共用
pub fn apply(plan: ProjectPlan, op: &mut Operation) -> Result<ProjectSummary, Msg>;
```

**决策表（plan 生成规则）**

| 现状 | 请求 | 决定 |
|------|------|------|
| 无 | 任意 | 创建父目录 + 按 `mode` 落点 |
| 已是本项目同 target 链接 | 任意 | `AlreadyPresent`（幂等） |
| 是链接但指向别处 | 任意 | `Backup`（记录旧目标）→ `RemoveLink` → 重建 |
| 是实体目录（未纳管） | 任意 | `E_DRIFTED`（不覆盖）；用户确认后 `Backup` → `RemoveTree` → 重建 |
| copy 模式且哈希与记录一致 | 升级 | `RemoveTree` → `CopyTree` |
| copy 模式且哈希不一致 | 升级 | `E_DRIFTED`；**不覆盖** |
| Windows 且无 symlink 权限 | symlink | 自动转 junction + `WARN E_LINK_PRIVILEGE` |

**与 npx skills 的共存（设计 §10.15，实现约束）**：对 `.agents/skills` 映射点，`plan_project` 只允许操作 `projection` 表中属于 Heron 的条目；发现同名的非受管实体时一律走 `E_DRIFTED`（先收拢再投影），不得覆盖。

**token 预估**：`token_estimate ≈ Σ(ceil((name+description) 字符数 / 3.5))`，按目标客户端分别汇总；批量启用前在界面展示（设计风险 R6）。

### 7.2 可见性判定（`verify`，设计 §10.11.1）

```rust
pub fn verify(db: &Db, scope: VerifyScope) -> Result<Vec<VisibilityReport>, Msg>;

pub enum Visibility { Visible, NotScanned, Shadowed { by: String }, RestartRequired, Broken }
```

判定顺序（离线完成，不调用客户端）：

1. `projection.state != ok` → `Broken`；
2. 落点路径不在该客户端的 `global_paths` / `project_paths` 内 → `NotScanned`；
3. 同目标存在同名且来自 plugin 的条目（读 `~/.claude/plugins/installed_plugins.json`、`<proj>/.claude/skills`）→ `Shadowed{by}`；
4. 落点创建时间晚于会话启动（能判断时）→ `RestartRequired`；
5. 否则 `Visible`。

结果写 `visibility` 表，GUI 状态图标直接读该表（设计 §12.3）。

### 7.3 原生扫描注入（`nativescan.rs`，D6）

```rust
pub struct NativeScanCapability {
    pub supported: bool,
    pub method: Option<String>,      // 例如 "extra_skill_dirs_setting"
    pub config_path: Option<PathBuf>,
    pub evidence: String,
}
pub fn status(db: &Db) -> Vec<(ClientId, NativeScanCapability, InjectState)>;
pub fn plan_inject(db: &Db, client: &ClientId) -> Result<InjectPlan, Msg>;   // 含 unified diff
pub fn apply_inject(plan: InjectPlan, op: &mut Operation) -> Result<(), Msg>;
pub fn revert(db: &Db, client: &ClientId) -> Result<(), Msg>;                // 从 backups/native-scan 还原
```

**硬规则**：

* `config.native_scan.enabled == false`（默认）→ `E_NATIVE_SCAN_DISABLED`；
* `capability.supported == false`（当前 5 个客户端均如此）→ `E_NATIVE_SCAN_UNSUPPORTED`，**不允许任何试写**；
* 支持后：备份 → 展示 diff → 用户确认 → 原子写 → 记 `event`；若文件哈希与上次记录不一致（用户手改）则拒绝覆盖并报 `E_DRIFTED`。

### 7.4 服务层用例（heron-service）

```rust
pub struct AdoptCommand { pub paths: Vec<PathBuf>, pub all: bool, pub cancel: CancelToken }
impl AdoptCommand {
    pub fn plan(&self, ctx: &Ctx) -> Result<AdoptPlan, Msg>;              // GUI 预览
    pub fn execute(&self, ctx: &Ctx, plan: AdoptPlan) -> Result<AdoptReport, Msg>;
}
```

| 用例 | 要点 |
|------|------|
| `ScanCommand` | 增量写 `detected`；病态嵌套只报告（D8） |
| `AdoptCommand` | 备份 → 迁入 Store → `git init` → 建投影；同哈希复用已有 slug |
| `InstallCommand` | URL/市场条目 → 解析 → 拉取 → 校验 → 写 Store → 可选投影 |
| `ProjectCommand` / `UnprojectCommand` | 7.1 计划 + 执行 |
| `UpgradeCommand` | fetch → 分类 → 逐条 5.5 合并 → 刷新投影 |
| `RollbackCommand` | `reset_to(tag)` + 刷新投影（先打备份标签） |
| `DoctorCommand` | 检查项注册表，每项产出 `Issue{ id, subject, severity, actions }` |
| `LockCommand` | 导出/导入 `heron.lock`（字段同设计 §7.5） |
| `LegacyCommandCommand` | 旧式命令文件 list / adopt / 回投影（D9） |

**进度事件**：长任务每 200 ms 或每 1% 发一次 `ProgressEvent{ task_id, phase, done, total, subject }`。

### 7.5 Tauri 命令面（`src-tauri/src/commands/*`）

```rust
#[tauri::command] fn scan(roots: Option<Vec<String>>) -> Result<ScanReportDto, Msg>;
#[tauri::command] fn detected_list(filter: DetectedFilter) -> Result<Vec<DetectedDto>, Msg>;
#[tauri::command] fn adopt(paths: Vec<String>, dry_run: bool) -> Result<AdoptReportDto, Msg>;
#[tauri::command] fn skills_list(q: ListQuery) -> Result<Page<SkillSummary>, Msg>;
#[tauri::command] fn skill_get(slug: String) -> Result<SkillDetail, Msg>;
#[tauri::command] fn project_bulk(ops: Vec<ProjectOp>) -> Result<Vec<ProjectResult>, Msg>;
#[tauri::command] fn unproject(slug: String, target: String, force: bool) -> Result<(), Msg>;
#[tauri::command] fn targets_list() -> Result<Vec<TargetDto>, Msg>;
#[tauri::command] fn target_add(path: String) -> Result<TargetDto, Msg>;
#[tauri::command] fn sources_list() -> Result<Vec<SourceDto>, Msg>;
#[tauri::command] fn source_add(input: String) -> Result<ParsedSourceDto, Msg>;
#[tauri::command] fn market_search(market: String, q: String) -> Result<Page<MarketItemDto>, Msg>;
#[tauri::command] fn market_install_plan(market: String, key: String) -> Result<InstallPlanDto, Msg>;
#[tauri::command] fn outdated() -> Result<Vec<UpgradeCandidate>, Msg>;
#[tauri::command] fn upgrade(sel: UpgradeSelection, dry_run: bool) -> Result<UpgradeReport, Msg>;
#[tauri::command] fn diff(slug: String, against: Option<String>) -> Result<DiffDto, Msg>;
#[tauri::command] fn merge_conflict(slug: String) -> Result<Vec<ConflictFile>, Msg>;
#[tauri::command] fn merge_resolve(slug: String, resolutions: Vec<Resolution>) -> Result<(), Msg>;
#[tauri::command] fn rollback(slug: String, to: String) -> Result<(), Msg>;
#[tauri::command] fn doctor_run(fix: bool) -> Result<DoctorReport, Msg>;
#[tauri::command] fn verify(slugs: Option<Vec<String>>) -> Result<Vec<VisibilityReport>, Msg>;
#[tauri::command] fn native_scan_status() -> Result<Vec<NativeScanDto>, Msg>;
#[tauri::command] fn native_scan_apply(client: String) -> Result<(), Msg>;
#[tauri::command] fn native_scan_revert(client: String) -> Result<(), Msg>;
#[tauri::command] fn lock_export(project: String, out: Option<String>) -> Result<String, Msg>;
#[tauri::command] fn lock_import(path: String, dry_run: bool) -> Result<LockReport, Msg>;
#[tauri::command] fn settings_get() -> Result<Config, Msg>;
#[tauri::command] fn settings_set(cfg: Config) -> Result<(), Msg>;
#[tauri::command] fn preflight() -> PreflightDto;
#[tauri::command] fn open_in_editor(slug: String) -> Result<(), Msg>;
#[tauri::command] fn task_cancel(task_id: String) -> Result<(), Msg>;
```

**事件**：`progress` / `scan:updated` / `skill:changed` / `source:probe` / `toast` / `log`。

**能力声明**（`capabilities/main.json`）：仅 `core:default`、`dialog:default`、`opener:default`、`updater:default`；**不得**开放 `fs:` 通配权限——所有文件操作走 Rust 侧。

### 7.6 CLI（heron-cli）

命令树与设计 §11 一致。实现要点：

* 写命令默认 `--dry-run` 输出计划表，`--yes` 跳过；
* `--json` 输出 DTO（与 GUI 同源）；退出码 0 成功 / 1 错误 / 2 用法 / 3 需用户决策 / 4 部分成功；
* 与 GUI 共存：先尝试 IPC（读 `ipc.json`），失败则 `platform::exclusive("db")` 后直接执行。

---

## 8. 前端（Vue 3）

### 8.1 目录与路由

```text
ui/src/
├── main.ts               # createApp + router + pinia + i18n + 全局错误处理
├── App.vue               # 侧边栏布局 + 底部任务条 + toast 容器
├── router.ts
├── api/invoke.ts         # 统一 invoke()：超时、错误解析、进度订阅
├── api/types.ts          # 与 Rust DTO 对应的 TS 类型（CI 校验一致性）
├── stores/               # pinia：app / skills / detected / sources / tasks / settings
├── views/
│   ├── OverviewView.vue      LibraryView.vue        SkillDetailView.vue
│   ├── DiscoverView.vue      SourcesView.vue        MarketBrowseView.vue
│   ├── ProjectsView.vue      UpgradesView.vue       DoctorView.vue
│   └── SettingsView.vue
├── components/
│   ├── SkillCard.vue         StatusBadge.vue        ModePicker.vue
│   ├── ProjectionTable.vue   DiffDrawer.vue         MergeConflictEditor.vue
│   ├── PlanPreview.vue       GuidedMarketFallback.vue  PasteUrlInput.vue
│   └── TaskBar.vue           EmptyState.vue
└── i18n/                 # §9
```

**路由表**：`/`、`/library`、`/library/:slug`、`/discover`、`/sources`、`/sources/:id/browse`、`/projects`、`/upgrades`、`/doctor`、`/settings/:tab?`。

### 8.2 状态与数据流

```text
invoke() ─▶ heron-service ─▶ SQLite
   ▲                 │
   └── event('progress' | 'skill:changed' | 'toast')
```

Pinia 只缓存**视图状态**（筛选、分页、选中项）；业务真值永远来自后端查询结果，不做前端乐观写入（避免与文件系统真值冲突）。

### 8.3 关键交互约定

| 场景 | 约定 |
|------|------|
| 任何写操作 | 先 `dry_run` → `PlanPreview` 展示将发生的动作 → 确认后执行 |
| 批量启用 | 展示 `token_estimate`（按客户端分组），超阈值 5000 时警示 |
| 市场 | `indexed` 显示"安装"；`guided` 显示"在浏览器打开 + 粘贴地址"（`GuidedMarketFallback`） |
| 升级冲突 | `MergeConflictEditor`：左 ours / 右 theirs / 中结果，逐块接受；二进制仅二选一 |
| 病态嵌套 | 只展示问题与三个动作（忽略 / 打开目录 / 导出诊断），**不提供"一键修复"**（D8） |
| 原生扫描 | 设置页默认关闭；开启需逐客户端，展示完整 diff 后才允许写入（D6） |

### 8.4 视觉与主题

* Tailwind 4 + CSS 变量令牌，亮/暗跟随系统；
* 状态色与设计 §12.3 一一对应；
* 表格用 `@tanstack/vue-virtual` 虚拟滚动，5000 行不卡顿。

---

## 9. 国际化与会话文案

### 9.1 数据流

```text
Rust: Msg { code: "E_MERGE_CONFLICT", params: { file: "SKILL.md" } }
        │
        ▼
Vue:  t("errors." + code, params)  →  本地化后的可读文案
```

* 语言包：`ui/src/i18n/locales/{zh-CN,en-US}/{common,skill,source,upgrade,doctor,errors}.json`；
* 语言选择：`general.language`（默认跟随系统）；
* CLI：`LANG` → `resources/i18n/{zh-CN,en-US}.toml`（由构建脚本从 JSON 生成，避免两份漂移）。

### 9.2 CI 校验（必须实现）

1. `zh-CN` 与 `en-US` 的 key 集合完全一致（缺失/多余均失败）；
2. 扫描 `crates/**/src/**/*.rs`：禁止中文字面量与长英文句子出现在 `Msg` 以外（日志需加 `// i18n-exempt` 注释）；
3. 每个 `ErrorCode` 常量在语言包中均有条目。

---

## 10. 质量保障

### 10.1 测试分层

| 层 | 范围 | 工具 | 目标 |
|----|------|------|------|
| 单元 | `heron-core` 全部、解析器、URL 解析、指纹 | `cargo test` + `proptest` | 行覆盖 ≥ 85% |
| 组件 | `heron-platform::link`、`heron-vcs`、`heron-projection` | `tempfile` 临时目录 | 覆盖 §10.2 场景矩阵 |
| 集成 | `heron-service` 用例（adopt/install/project/upgrade/rollback） | 本地 bare 仓库夹具 | 每条用例含失败注入 |
| 端到端 | GUI 主流程 | Playwright（Tauri 驱动，`pnpm test:e2e`） | 见 §10.4 |
| 手工 | 真机客户端可见性 | 清单（§10.5） | 发布前必过 |

### 10.2 Windows 链接层测试矩阵（风险 R3 的守门员）

```rust
#[cfg(windows)]
mod windows_link {
    // 每个用例都在临时目录中创建 store 目标，写入哨兵文件后执行，最后断言哨兵仍在
    #[test] fn create_junction_then_remove_keeps_target()        { /* … */ }
    #[test] fn create_symlink_dir_when_privileged_else_falls_back(){ /* … */ }
    #[test] fn probe_distinguishes_junction_from_symlink()       { /* … */ }
    #[test] fn remove_link_on_real_dir_is_refused()              { /* … */ }
    #[test] fn relink_replaces_stale_target()                    { /* … */ }
    #[test] fn long_path_beyond_260_roundtrips()                 { /* … */ }
}
```

| 序号 | 场景 | 断言 |
|------|------|------|
| L1 | 创建 junction 后删除 | 目标内容 100% 保留（哨兵文件存在） |
| L2 | 普通用户创建 symlink 失败 | 自动降级为 junction，返回 `WARN E_LINK_PRIVILEGE` |
| L3 | `probe` 对 junction / symlink 的判定 | 分别返回 `Junction` / `SymbolicDir`，不混淆 |
| L4 | 对实体目录调用 `remove_link` | 返回 `E_SAFETY_REFUSED`，目录未被删除 |
| L5 | 超过 260 字符的路径 | 创建/探测/删除全部成功 |
| L6 | 目标被手工删除后再 `verify` | 报 `Broken`，不崩溃 |

> **CI 硬门禁**：L1–L6 在 `windows-2022` 的**普通用户**上下文中必须全绿，否则禁止合入。

### 10.3 关键集成用例（`tests/e2e/`）

| 用例 | 步骤 | 断言 |
|------|------|------|
| adopt-dedup | 三个位置放同名同内容 Skill | 收拢后 store 仅 1 份，3 处均为链接，备份目录含原文件 |
| adopt-drifted | 同名不同内容 | 报告为 `conflict`，不静默覆盖 |
| legacy-command | `.claude/commands/review.md` | 生成 `store/review/SKILL.md`，原文件保留，体检标记 legacy |
| install-from-subdir | GitHub 子目录 URL | 正确解析 ref/subpath 并落库 |
| upgrade-clean | 本地无改动 | 升级成功，版本 tag 增加，投影刷新 |
| upgrade-merge | 本地改一行、上游改另一行 | 三方合并成功且两处都在 |
| upgrade-conflict | 双方改同一行 | 返回冲突清单，store 不变（可回滚验证） |
| upgrade-online-interrupt | 合并中途注入 kill | 重启后状态一致性检查通过 |
| project-mode-switch | link → copy → pointer | 三种模式可互相切换，旧链接被安全移除 |
| copy-drift | copy 后手工改目标 | 报告 `modified`，升级时不覆盖 |
| nested-report-only | 病态嵌套样本 | `doctor` 只报告，文件系统零变化（哈希前后一致） |
| lock-roundtrip | 导出 → 删除 store → 导入 | 复现同一内容与版本 |
| native-scan-guard | 默认配置下开启注入 | 返回 `E_NATIVE_SCAN_DISABLED`，无任何文件写入 |

### 10.4 端到端（GUI）

Playwright 覆盖 6 条主路径：首次引导（含 Git 缺失引导）→ 发现与收拢 → 启用投影 → 市场搜索与安装 → 升级与冲突解决 → 体检与修复。每条路径在 macOS 与 Windows 各跑一次。

### 10.5 发布前手工清单（真机）

1. Claude Code：新建链接后**新开会话**，`/skills` 能看到；同名被插件压制时 `verify` 报 `shadowed`；
2. Codex：`~/.agents/skills` 映射点开启后可见；
3. 两个客户端各改一个 Skill 的 Store 内容，刷新后可见（验证链接而非副本）；
4. Windows 普通用户：全流程无 UAC 提示；
5. 原生扫描能力表：逐客户端确认"不支持"结论是否仍成立，更新 `evidence`。

### 10.6 错误码清单（`heron-core::error`）

| 域 | 错误码 |
|----|--------|
| 环境 | `E_GIT_MISSING`、`E_GIT_UNSUPPORTED`（< 2.38 但走合并）、`E_WEBVIEW_MISSING`、`E_PERMISSION` |
| 扫描 | `E_SCAN_LIMIT`、`E_SCAN_ROOT_MISSING`、`E_FRONTMATTER` |
| Slug / 冲突 | `E_SLUG_CONFLICT`、`E_SLUG_INVALID`、`E_RESERVED_NAME` |
| 链接 | `E_LINK_PRIVILEGE`、`E_LINK_TARGET_MISSING`、`E_SAFETY_REFUSED` |
| 投影 | `E_DRIFTED`、`E_NOT_MANAGED`、`E_TARGET_NOT_FOUND` |
| Git / 网络 | `E_NETWORK`、`E_AUTH`、`E_REF_NOT_FOUND`、`E_CLONE_TIMEOUT` |
| 来源 | `E_SOURCE_AMBIGUOUS`、`E_CATALOG_PARSE`、`E_MARKET_NO_GIT_SOURCE`、`E_MARKET_UNAVAILABLE` |
| 升级 | `E_MERGE_CONFLICT`、`E_UPSTREAM_GONE`、`E_PINNED`、`E_DIRTY_WORKTREE` |
| 原生扫描 | `E_NATIVE_SCAN_DISABLED`、`E_NATIVE_SCAN_UNSUPPORTED`、`E_NATIVE_SCAN_DRIFT` |
| 系统 | `E_DB_CORRUPT`、`E_CANCELLED`、`E_INTERNAL` |

> 每个错误码必须同时提供：中英文案、`retryable` 标记、`user_actions`（可点的下一步）。

---

## 11. 实施顺序（按文件粒度）

> 原则：**每个里程碑结束时主干可运行、可演示、有测试**，不做"大爆炸式集成"。

### M0 地基（约 1.5 周）

| 顺序 | 动作 | 产出 |
|------|------|------|
| 1 | workspace 骨架、`rust-toolchain.toml`、CI 三平台 | `cargo test` 空跑通过 |
| 2 | `heron-core`：slug / version / policy / error / dto | 单测 + proptest |
| 3 | `heron-platform`：dirs / paths / atomic / lock / process / probe | Windows 与 macOS 单测 |
| 4 | `heron-store`：db / migrations / repos | 迁移测试（空库 → 最新） |
| 5 | `heron-scan`：walker / rules / parse / classify / fingerprint | 夹具用例 |
| 6 | `heron-cli` 最小命令集：`scan`、`list`、`detected` | 本机 4 类目录全部识别 |
| 7 | `heron-app` 骨架 + Vue 空壳 + `preflight` 命令 + i18n 骨架 | GUI 可启动并显示语言切换 |
| 8 | Git preflight 与安装引导页 | 未装 git 时出引导（D7） |

**M0 DoD**：本机 ≥ 400 个 `SKILL.md` 全部入索引；扫描 5 万目录 < 10 s；中英文 key 一致。

### M1 收拢与分发（约 2 周）

| 顺序 | 动作 |
|------|------|
| 1 | `heron-platform::link` 全平台实现 + §10.2 测试矩阵 |
| 2 | `heron-vcs`：init / commit / tag / reset |
| 3 | `heron-projection`：plan / apply / verify |
| 4 | `heron-service`：Adopt / Project / Unproject / Doctor(基础项) |
| 5 | 默认落点 `~/.heron/skills` 与映射点 `~/.agents/skills`（D10） |
| 6 | `legacy-command` 识别与收拢（D9） |
| 7 | GUI：发现页、库页、详情页、投影开关、计划预览 |
| 8 | `heron-cli` 对应命令 + `--dry-run` |

**M1 DoD**：本机 3 处重复 Skill 收拢为 1 份；默认落点生效；`.claude/commands` 可收拢且原文件保留；`verify` 四类结论正确；Windows 普通用户全流程通过。

### M2 远程、市场与升级（约 2.5 周）

| 顺序 | 动作 |
|------|------|
| 1 | `heron-git`：裸仓缓存、探针、稀疏检出 |
| 2 | `heron-source::url`：四种形态 + GitLab 歧义探测 |
| 3 | `heron-source::catalog`：marketplace.json / heron.source.toml / 兜底扫描 |
| 4 | 市场适配器：`skills_sh` → `clawhub` → 三个 Guided |
| 5 | `heron-service`：Install / Outdated / Upgrade / Rollback / Diff |
| 6 | `heron-vcs::merge3`：merge-tree 优先 + similar 兜底 + 冲突清单 |
| 7 | GUI：来源页、市场浏览页、更新页、差异抽屉、冲突编辑器 |
| 8 | 离线降级与缓存过期提示 |

**M2 DoD**：从 GitHub 子目录安装 → 本地改一行 → 上游更新 → 三方合并成功 → 回滚；断网/中断不留半态；五个市场各有可演示路径。

### M3 体检、原生扫描与团队（约 1.5 周）

| 顺序 | 动作 |
|------|------|
| 1 | `doctor` 全部检查项（含病态嵌套只报告，D8） |
| 2 | `visibility` 判定接入 GUI 状态图标 |
| 3 | `nativescan`：状态、能力表、diff 预览、注入与还原（默认关，D6） |
| 4 | `heron.lock` 导出/导入 + `sync` |
| 5 | GUI：体检页、项目页、设置页（含原生扫描 tab） |

**M3 DoD**：同一锁文件在 macOS 与 Windows 复现同一套 Skill；病态嵌套样本零改动；原生扫描注入前展示 diff 且可一键还原。

### M4 打磨与发布（约 1 周）

自动更新（Tauri updater + 签名）、诊断包导出、崩溃恢复（`Operation` 残留扫描）、性能优化、两语言全量校对、安装包签名与公证。

---

## 12. v1.0 验收清单（映射设计 §14）

- [ ] `heron scan` 在本机识别 4 类目录，误报 < 5%
- [ ] 收拢后同一 Skill 仅存一份实体，原位置为投影，备份可恢复
- [ ] macOS / Windows 普通用户均可完成「发现 → 收拢 → 启用」
- [ ] 默认落点 `~/.heron/skills` 生效；`~/.agents/skills` 可作映射点开启
- [ ] `verify` 正确区分 `visible / not_scanned / shadowed / restart_required / broken`
- [ ] `outdated` 误报为 0；`upgrade` 保留本地改动；`rollback` 可回滚
- [ ] 五个内置市场：2 个可安装、3 个引导式，均可离线降级
- [ ] `doctor` 对病态嵌套只报告、不修改
- [ ] `.claude/commands` 可收拢且原文件保留
- [ ] 无 Git 时给出安装引导，不静默降级
- [ ] 中英文界面完整；无硬编码文案
- [ ] Windows 链接测试矩阵 L1–L6 全绿

---

## 13. 交付与运维

| 项 | 内容 |
|----|------|
| 版本号 | GUI 与 CLI 同版本号；`heron --version` 同时输出 store schema 版本 |
| 更新通道 | Tauri updater（stable 渠道）+ 手动检查；更新前备份 `heron.db` |
| 日志 | `<DATA>/log/heron.log`，按天滚动，保留 14 天；诊断包 = 日志 + 配置（脱敏）+ 索引统计 |
| 隐私 | 无遥测；网络仅用于：Git 操作、市场接口、更新检查 |
| 卸载 | 卸载不删除 `<DATA>`；界面提供"彻底清除"（二次确认 + 先导出备份） |
| 故障恢复 | `heron doctor --repair`：重建索引、清理残留 `Operation`、修复坏链 |

---

## 14. 风险落地对照（设计 §15）

| 风险 | 本文落点 |
|------|----------|
| R1 扫描路径/优先级变动 | `resources/clients/*.toml` 数据化 + `verified_at`；§10.5 手工复核清单 |
| R2 扫描器不识别链接 | `ProjectionMode` 可切换；`pointer` 兜底；GUI 在 `verify` 为 `not_scanned` 时提示 |
| R3 junction 误删 | §4.3 唯一出口 + §10.2 测试门禁 |
| R6 上下文膨胀 | §7.1 `token_estimate` + 批量启用警示 |
| R7 双头管理 | `detected`/`projection` 的 managed 边界；`.agents` 只碰自己创建的条目（§7.1） |
| R11 原生配置被改坏 | §7.3 硬规则（默认关、不支持不写、可还原） |
| R12 默认落点不被扫描 | 首次引导调用 `verify` 并提示"是否同时投影到客户端目录" |

---

## 15. 开工检查清单

1. 仓库初始化：workspace 骨架 + CI（三平台矩阵）；
2. 确认 `rust-toolchain.toml` 钉住版本，且在 Windows `cargo test -- --ignored` 可跑；
3. 复刻 §10.2 的 L1 用例作为**第一段提交**（先立安全网）；
4. 确认 `clients/*.toml` 的 `evidence` 字段在首次导入 DB 时可见；
5. 建立 i18n 校验脚本并在 CI 中阻断（避免后期返工）；
6. 按 §11 的 M0 顺序逐条推进，每个 PR 必须包含对应测试。

> 本文与设计文档如有冲突，以**设计文档为准**并回写本文；新增技术决策写入 `docs/adr/`。

---

## 16. 落地进度（滚动更新）

| 日期 | 里程碑 | 已落地的代码 | 验证方式 |
|------|--------|--------------|----------|
| 2026-11-19 | M0（部分） | `heron-core`：slug / version / policy / error / dto / hash；`heron-platform`：dirs / paths / atomic / lock / link（unix + windows）/ probe | `cargo test --workspace` 81 例通过；`cargo fmt --check`、`cargo clippy -D warnings` 干净；`--target x86_64-pc-windows-msvc` 静态编译通过 |
| 2026-11-19 | M0（部分） | 链接层安全测试：`tests/link_safety.rs`（10 例，全平台）+ `tests/link_safety_windows.rs`（L1–L6，Windows 门控） | macOS 实跑 10/10；Windows 用例待 Windows CI 或真机执行 |
| 2026-11-20 | 发布签名 | Tauri 更新签名（minisign 密钥对）接入：`createUpdaterArtifacts`、updater 插件与权限、设置页更新入口、`scripts/make-latest-json.py`、release/publish 两个 CI 任务（缺私钥即失败、缺签名即失败） | 本地实测：构建产出 `Heron.app.tar.gz.sig`，用公钥**密码学验证通过**，篡改后正确拒绝；229 项测试、clippy、typecheck、i18n 全绿 |
| 2026-11-19 | Windows 实测修复 | 仓库推送到 GitHub 后 CI 在真实非提权 Windows runner 上跑 L1–L6，连续暴露 4 个链接层缺陷（GENERIC_WRITE 读取、symlink PathBuffer 偏移、NT 前缀转义、悬空链接分类与删除）并全部修复；另修 2 处测试平台假设 | Windows **229 项全绿**（含此前从未真跑的 L1–L6）；Ubuntu 同样通过 |
| 2026-11-19 | 收尾功能 | GUI 三栏冲突编辑器（Mine/Theirs + 逐文件决策 → 保存并重试升级）、`native_scan` 能力表驱动注入（默认关闭、先展示 diff、可还原）、设置页显示各客户端能力与依据 | 228 例测试全绿；`vue-tsc` 通过；中英 126 个 i18n key 一致；`pnpm build` 171 KB |
| 2026-11-19 | 交付准备 | 图标与 bundle 配置（dmg / NSIS）、GitHub Actions（三平台测试 + Windows 非提权链接门禁 + 标签触发打包）、`heron diff` / `conflict` / `resolve` | 224 例测试全绿；真机演练：本地改动 diff → 上游改同一行 → conflict 列出文件 → resolve --take theirs 完成升级；CI 配置已就位（需推送到 GitHub 才实际运行） |
| 2026-11-19 | M4（GUI 首版） | `src-tauri`（Tauri 2 外壳 + 20 个 command，全部委托 service）+ `ui/`（Vue 3 + TS + Pinia + Router + vue-i18n，7 个页面） | `cargo build -p heron-app` 通过；`vue-tsc` 无错误；`pnpm build` 产出 171 KB（gzip 64 KB）；中英 114 个 i18n key 一致；应用可正常启动并初始化数据目录 |
| 2026-11-19 | M3（核心完成） | 市场适配器（skills.sh sitemap 索引、ClawHub REST、三家引导式）、`heron.lock` 导出/差异/复现、项目自动发现（默认关闭投影）、客户端矩阵外置为 `resources/clients/*.toml` | 273 例测试全绿；实测：skills.sh 解析出 `anthropics/skills/pdf` → GitHub 安装计划，ClawHub 搜索可用但全部为平台托管（引导式）；`heron.lock` 在新数据目录一键复现并检出内容漂移 |
| 2026-11-19 | M2（核心完成） | `heron-git`（每 URL 一个裸仓缓存、ref 解析、`git archive` 子目录检出、tar 解包含路径穿越防护）、`heron-source`（四种 URL 形态 + GitLab 歧义候选 + catalog 三级优先）、service `install/outdated/upgrade/rollback`、CLI 同名命令 | 209 例测试全绿；真机演练（本地 bare 远端）：安装 → 本地改一行 → 上游发布 1.2.0 → 三方合并保留双方改动 → 标签 v1.1.0/v1.2.0 → 回滚成功 |
| 2026-11-19 | M1（核心完成） | `heron-vcs`（每 Skill 独立 Git 仓库、`merge-tree` 三方合并、回滚标签）、`heron-projection`（plan/apply/verify + 三种投影互转）、`heron-service`（scan/adopt/project/unproject/verify/doctor）、CLI 接入 `adopt/project/unproject/verify/doctor/targets` | 174 例测试全绿；真机演练：2 份 pdf 收拢为 1 份 store、两处原位置变链接且 `verify` 均判 visible；legacy 命令只接管单个文件；`project --on claude:global` 可用 |
| 2026-11-19 | M0（完成） | `heron-store`：迁移 0001/0002 + 6 个仓储 + 补偿事件表；`heron-scan`：walker / rules / parse / classify / fingerprint；`heron-cli`：`scan` / `list` / `detected` | 131 例测试全绿；真机扫描 219 目录 / 52 个 skill（79 ms）；`--roots` 下的 `.claude/commands` 被识别为 legacy-command |

**实现中产生的决策**见 `docs/adr/`：

- `0001-cargo-home-and-toolchain-isolation.md`：构建缓存与工具链隔离在工作区内；
- `0002-drop-blake3-from-platform.md`：平台层路径指纹改用 FNV-1a，blake3 只用于内容指纹；
- `0003-link-safety-contract.md`：链接层的五条安全契约，由测试矩阵兜底；
- `0004-data-dir-override-owns-everything.md`：显式数据目录拥有 config/cache/log；
- `0005-storage-holds-real-paths.md`：数据库只存真实路径，`display()` 仅用于渲染；
- `0006-projection-identity-and-targets.md`：投影按"位置"标识，客户端目录自动成为目标；
- `0007-bookkeeping-ignored-by-identity.md`：Heron 记账文件不参与内容指纹；
- `0008-upstream-objects-and-version-tags.md`：升级导入上游对象、`--merge-base` 传 tree、版本标签打在本地提交；
- `0009-markets-and-lock.md`：市场适配分可搜索/仅浏览器两级，`heron.lock` 不记绝对路径；
- `0012-windows-link-faults.md`：四个只在真实 Windows 暴露的链接层缺陷与排查方法；
- `0010-gui-shell-thin-adapter.md`：GUI 是薄壳，逻辑全在 service，CLI 与 GUI 行为一致；
- `0011-diff-conflict-and-packaging.md`：diff 以本地基线为准、未提交改动计入、缓存抓取加深、冲突解决后重试升级。

**剩余待办**：

1. 把仓库推到 GitHub 并让 CI 实际跑一次（Windows 链接门禁 L1–L6 尚未在真机验证）；
2. 代码签名与公证（需要 Apple Developer ID 与 Windows 证书）；
3. 为某个客户端真机验证原生扫描配置格式后，在 `clients/*.toml` 里把 `native_scan.supported` 改为 true。
