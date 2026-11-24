# 修复「来源与市场 → 我的收藏」安装卡死

## Context（为什么做这件事）

用户反馈：收藏里的 skill（`https://github.com/alchaincyf/huashu-design`）点「安装」后一直转圈、从未成功。

调用链：[installFavorite](file:///Users/jingyuan-mb01/javaspace/REA/heron/ui/src/views/SourcesView.vue#L269-L278) → `api.install` → [install_skill](file:///Users/jingyuan-mb01/javaspace/REA/heron/src-tauri/src/lib.rs#L765-L778) → [install::install](file:///Users/jingyuan-mb01/javaspace/REA/heron/crates/heron-service/src/install.rs#L205-L220) → [resolve_remote](file:///Users/jingyuan-mb01/javaspace/REA/heron/crates/heron-service/src/install.rs#L62-L100) → [CachedRepo::ensure](file:///Users/jingyuan-mb01/javaspace/REA/heron/crates/heron-git/src/cache.rs#L43-L82) → [Git::run](file:///Users/jingyuan-mb01/javaspace/REA/heron/crates/heron-vcs/src/git.rs#L113-L131) → `git fetch`。

### 实测证据（本机 macOS）

`scutil --proxy` 显示系统代理 `HTTPProxy=127.0.0.1 HTTPPort=7897`（Clash Verge）。git 在 macOS 上**不读系统代理**，只认 env 与 `http.proxy` 配置，因此 Heron 的 `git fetch` 实际是直连 GitHub：

| 命令 | 结果 |
| --- | --- |
| `git fetch --depth=200 --tags <repo> '+refs/heads/*:refs/remotes/origin/*'` | **6 分 44 秒**后 `RPC failed; curl 56 Recv failure: Connection reset by peer` / `early EOF`，未写入任何 ref |
| 同命令 + `-c http.proxy=http://127.0.0.1:7897` | **18 秒成功**，refs 齐全 |
| `git ls-remote https://github.com/...`（直连） | `Recv failure: Operation timed out` |
| `curl -x http://127.0.0.1:7897 https://github.com/` | 200 |

现网缓存 `~/Library/Application Support/xin.v5ai.heron/sources-cache/f1863739b2408095/bare.git` 正是失败残骸：10M objects、`refs/` 为空、`FETCH_HEAD` 为空。

放大问题的四个缺陷：

1. `NETWORK_TIMEOUT` 在 [heron-vcs/git.rs:133](file:///Users/jingyuan-mb01/javaspace/REA/heron/crates/heron-vcs/src/git.rs#L133)（60s）与 [heron-git/cache.rs:12](file:///Users/jingyuan-mb01/javaspace/REA/heron/crates/heron-git/src/cache.rs#L12)（120s）都是**死常量**，`run_capture` 用阻塞 `cmd.output()`，永不超时。
2. 失败后 [cache.rs:66-75](file:///Users/jingyuan-mb01/javaspace/REA/heron/crates/heron-git/src/cache.rs#L66-L75) 再用「去掉 `--depth`」全量重试一次 → 最坏等待翻倍（约 13 分钟）。
3. 整个网络阶段在 [lib.rs:767](file:///Users/jingyuan-mb01/javaspace/REA/heron/src-tauri/src/lib.rs#L765-L778) 持有的 `state.ctx.lock()` 内完成，全仓 26 处 `ctx.lock()` 共用这把锁 → 一次慢安装把**整个界面**冻死（表现为「卡住」而非「报错」）。
4. 代理之外的连带问题：ureq（[market.rs:565](file:///Users/jingyuan-mb01/javaspace/REA/heron/crates/heron-source/src/market.rs#L565-L585)，ureq 2 默认不含 `proxy-from-env`）不读任何代理；`fetch` 不创建 `refs/remotes/origin/HEAD`，[default_branch()](file:///Users/jingyuan-mb01/javaspace/REA/heron/crates/heron-git/src/cache.rs#L138-L156) 兜底按字典序取第一个分支——huashu-design 会装成 `auto-optimize/20260420-…` 而非 `master`（已实测确认 refspec 追加 `+HEAD:refs/remotes/origin/HEAD` 可拿到 master）。

目标：安装能真正成功（走代理）、失败要快且可读（超时）、慢操作不再冻结 UI；代理可在设置页手动覆盖。

## 实施步骤

### 1. `crates/heron-platform/src/proxy.rs`（新增，lib.rs 注册 `pub mod proxy;`）

```rust
pub fn set_override(Option<String>);            // 设置页手动值，RwLock<Option<String>>
pub fn resolve() -> Option<String>;             // 生效代理，git/ureq 共用
pub fn parse_scutil(&str) -> Option<String>;    // 纯函数，可单测
```

优先级：① 手动 override → ② env（`HTTPS_PROXY/https_proxy/ALL_PROXY/all_proxy/HTTP_PROXY/http_proxy`，命中即返回 `None` 让 git/curl 自己用，避免重复注入）→ ③ 用户 `git config --get http.proxy`（存在则尊重、返回 `None`）→ ④ 仅 `#[cfg(target_os = "macos")]` 执行 `scutil --proxy`，按 `HTTPSEnable/HTTPProxy/HTTPPort` → `HTTPEnable` → `SOCKSEnable` 顺序拼成 `http://h:p` / `socks5://h:p`。

- 沿用 [probe.rs:69 detect_git](file:///Users/jingyuan-mb01/javaspace/REA/heron/crates/heron-platform/src/probe.rs#L69-L91) 的「`Command::output()` + `from_utf8_lossy` + 纯函数解析」范式；`OnceLock` 缓存 scutil 结果（override 变化时不缓存）。
- 不做 Windows 注册表 / Linux gsettings / PAC：当前只验证 macOS，收益低；在注释中写明「PAC 不支持」。

### 2. `crates/heron-vcs/src/git.rs`：真实超时

- 新增 `pub fn run_network(&self, cwd, args, timeout: Duration) -> Result<GitOutput>`：`spawn()` + `Stdio::piped()`，stdout/stderr 各起一条读线程累积到 `Arc<Mutex<Vec<u8>>>`（**不能串行 `read_to_string`，大输出会死锁**），主线程轮询 `child.try_wait()`；超时 `kill()` + `wait()`，返回 `ErrorCode::CloneTimeout`（[error.rs:105](file:///Users/jingyuan-mb01/javaspace/REA/heron/crates/heron-core/src/error.rs#L105) 已定义、目前零产生）并带 `url`/`detail` params。
- 抽出 `fn wait_timeout(&mut Child, Duration) -> io::Result<Option<ExitStatus>>` 便于单测（`/bin/sh -c 'sleep 5'`）。
- 代理以 `-c` 参数注入（集中到纯函数 `network_config_args(proxy: Option<&str>) -> Vec<String>`，可断言）：`-c http.proxy=<p>` / `-c https.proxy=<p>`（socks5 用 `socks5://`）、`-c http.lowSpeedLimit=1024 -c http.lowSpeedTime=20` 让 curl 自己早退。选 `-c` 而非子进程 env：只作用于这条命令、不污染 credential helper、可单测。
- `NETWORK_TIMEOUT` 语义落地为「单次网络调用预算」；保留现有本地 `run`/`run_checked` 行为不变（`repo.rs:322` 的 fetch 目标是本地 bare 路径，不加超时）。

### 3. `crates/heron-git/src/cache.rs`

- `ensure()` 首次与重试都改用 `run_network(..., NETWORK_TIMEOUT)`。
- refspec 追加 `'+HEAD:refs/remotes/origin/HEAD'`；`default_branch()` 仍优先该 symref，兜底改为先找 `main`/`master`/`develop`，最后才取字典序（兼容旧缓存没有该 ref）。
- 取消「无脑全量重试」：仅当首次 stderr 命中 `shallow|unshallow|Server does not allow` 时才去掉 `--depth` 重试（仍带超时）；网络/超时类失败直接返回，避免等待翻倍。
- 新增 `pub fn with_git(cache_root, remote, git: Git)`（现在 [cache.rs:27-34](file:///Users/jingyuan-mb01/javaspace/REA/heron/crates/heron-git/src/cache.rs#L27-L34) 没有注入口），供超时/失败路径单测。

### 4. `crates/heron-service/src/install.rs` + `src-tauri/src/lib.rs`：网络阶段移出全局锁

- `resolve_remote(ctx, req)` 改为 `resolve_remote(dirs: &Dirs, req)`（只用到 `dirs.sources_cache()`）；新增 `pub fn prefetch(dirs: &Dirs, req) -> Result<()>` 只做 `parse_source` + `ensure()`。
- `preview(&Ctx, ..)` / `install(&mut Ctx, ..)` 公开签名保持不变（[heron-cli app.rs:696/728](file:///Users/jingyuan-mb01/javaspace/REA/heron/crates/heron-cli/src/app.rs#L696) 与 `tests/remote_flow.rs` 都在用）；`Db` 是单连接，仍留在锁内。
- `install_skill` 与 `preview_install` 闭包内改为：短锁 clone `dirs` → **锁外** `prefetch` → 再取锁执行原 `preview`/`install`。错误原样透传，不改错误码语义。
- 不做取消按钮/进度事件（本次不扩范围）。

### 5. 市场 HTTP 走同一代理

`crates/heron-source/Cargo.toml` 增加 `heron-platform` 依赖（无环），[http_get_once](file:///Users/jingyuan-mb01/javaspace/REA/heron/crates/heron-source/src/market.rs#L565-L585) 里对 ureq AgentBuilder 追加 `.proxy(ureq::Proxy::new(&p)?)`（`resolve()` 有值时）。这样 skills.sh / clawhub 等海外源搜索不再必然超时。

### 6. 设置页代理项（前端 + 后端）

- 后端两个命令：`net_get_proxy` / `net_set_proxy(proxy: Option<String>)`，用现有 [db.kv_get / kv_set](file:///Users/jingyuan-mb01/javaspace/REA/heron/crates/heron-store/src/db.rs#L147-L162) 持久化键 `network.proxy`，并立刻 `proxy::set_override(...)`；`Ctx::from_data_dir`（[lib.rs:156](file:///Users/jingyuan-mb01/javaspace/REA/heron/src-tauri/src/lib.rs#L156)）建上下文时读该键回填 override，保证重启后生效。
- 前端：`api/index.ts` 加 `netGetProxy/netSetProxy`；[SettingsView.vue 系统分组](file:///Users/jingyuan-mb01/javaspace/REA/heron/ui/src/views/SettingsView.vue#L308-L316)（git 版本/平台/主目录下方）加一个「代理」输入框 + 「自动」留空语义（空 = 自动探测：env → git config → macOS 系统代理），保存即调用 `netSetProxy`。
- i18n：`ui/src/i18n/locales/{zh-CN,en-US}.json` 同步新增 `settings.proxy` / `settings.proxyHint` 与 `errors.E_CLONE_TIMEOUT`（zh 建议「下载超时，请检查网络或代理设置后重试」），过 `pnpm --dir ui i18n:check`。

## 风险与取舍

- `-c` 注入优先级高于用户 global config，所以第 1 步必须先探 `git config --get http.proxy` 并在其存在时不注入。
- 不识别 `no_proxy` 例外列表（本机无需求）；PAC 脚本不支持；非 macOS 只走 env / git config / 手动值。
- scutil 探测按进程缓存一次：Clash 换端口后需重启应用或改设置页手动值。
- `CloneTimeout` 与 `Network` 是两条文案，需同时补 zh/en（`i18n:check` 只校验两侧对称）。
- 慢代理 + 大仓库（huashu-design depth=200 实测 241M）仍可能超过单次预算：届时是清晰的「下载超时」而不是永久转圈；深度策略本次不动。
- `127.0.0.1:1` 立即拒连必须仍是 `E_NETWORK`（不能被误判为超时）——这是回归护栏，见 [cache_flow.rs:246](file:///Users/jingyuan-mb01/javaspace/REA/heron/crates/heron-git/tests/cache_flow.rs#L240-L255)。

## 验证清单

1. 单测/集成：`cargo test -p heron-platform -p heron-vcs -p heron-git -p heron-service -p heron-source`，覆盖 `parse_scutil`、`network_config_args`、`wait_timeout` 真杀进程、`default_branch` 解析、拒连仍为 `E_NETWORK`。
2. 新增 `crates/heron-git/tests/live_fetch.rs`（`#[ignore = "network"]`，仿 [live_markets.rs](file:///Users/jingyuan-mb01/javaspace/REA/heron/crates/heron-source/tests/live_markets.rs#L3-L10)）：真拉 huashu-design，断言 `refs/remotes/origin/HEAD` 解析为 `master`。
3. `cargo clippy --workspace --all-targets -- -D warnings`、`cargo build -p heron-app`。
4. CLI 端到端：`cargo run -p heron-cli -- install https://github.com/alchaincyf/huashu-design --dry-run`（应给出 slug/commit，**30 秒内**）→ 去掉 `--dry-run` 真装 → `heron skills` 确认落库；再关掉 Clash 重跑，应在超时预算内报「下载超时」。
5. 界面：删掉 `~/Library/Application Support/xin.v5ai.heron/sources-cache` 残骸后 `pnpm --dir ui tauri dev`，在「我的收藏」点安装的同时切换标签页/滚动——界面必须保持响应；成功后技能列表出现该 skill，失败时 toast 是可读错误码文案。
6. 设置页填 `http://127.0.0.1:7897` 与清空两种情况各验一次安装/搜索；`pnpm --dir ui i18n:check` + `typecheck` 通过；深浅主题下新增输入框正常。
