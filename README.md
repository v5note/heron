# Heron (苍鹭)

<p align="center">
  <img src="grey-heron-苍鹭.webp" alt="Heron Logo" width="160" />
</p>

<p align="center">
  <strong>Gather the Agent Skills scattered across your machine into one versioned, single source of truth, project them where your tools actually look, and make upgrading a single command.</strong>
</p>

<p align="center">
  <img src="https://img.shields.io/badge/License-MIT-blue.svg" alt="License MIT" />
  <img src="https://img.shields.io/badge/Rust-1.80%2B%20(2021)-orange.svg" alt="Rust Version" />
  <img src="https://img.shields.io/badge/Tauri-v2-24c8db.svg" alt="Tauri 2" />
  <img src="https://img.shields.io/badge/Platform-macOS%20%7C%20Windows%20%7C%20Linux-brightgreen.svg" alt="Platform" />
  <img src="https://img.shields.io/badge/Git-2.38%2B%20Required-red.svg" alt="Git Required" />
</p>

<p align="center">
  English · <a href="README.zh-CN.md">中文文档</a>
</p>

<p align="center">
  <picture>
    <source media="(prefers-color-scheme: dark)" srcset="images/首页-深色.png" />
    <source media="(prefers-color-scheme: light)" srcset="images/首页-浅色.png" />
    <img alt="Heron Overview" src="images/首页-浅色.png" width="920" />
  </picture>
</p>

---

## Table of Contents

- [Why Heron?](#why-heron)
- [Core Workflow: Adopt · Project · Upgrade](#core-workflow-adopt--project--upgrade)
- [Interface Showcase](#interface-showcase)
- [System Architecture & Workspace Crates](#system-architecture--workspace-crates)
- [Core Mechanisms & Technical Highlights](#core-mechanisms--technical-highlights)
  - [1. Single Source of Truth & 4 Projection Modes](#1-single-source-of-truth--4-projection-modes)
  - [2. Per-Skill Git Repositories & Three-Way Merge Upgrades](#2-per-skill-git-repositories--three-way-merge-upgrades)
  - [3. Dynamic Presence Detection for 13 AI Clients](#3-dynamic-presence-detection-for-13-ai-clients)
  - [4. Unified Search Across 6 Built-in Marketplaces & Favourites](#4-unified-search-across-6-built-in-marketplaces--favourites)
  - [5. Team Collaboration & Lockfile](#5-team-collaboration--lockfile)
- [Quick Start](#quick-start)
  - [Requirements](#requirements)
  - [Install](#install)
  - [Three Commands to Get Going](#three-commands-to-get-going)
- [Desktop App Navigation](#desktop-app-navigation)
- [CLI Command Overview](#cli-command-overview)
- [Design Principles & Safety Contracts](#design-principles--safety-contracts)
- [Where Data Lives & Storage Isolation](#where-data-lives--storage-isolation)
- [Development & Contributing](#development--contributing)
  - [Local Development](#local-development)
  - [Debugging Cheat Sheet](#debugging-cheat-sheet)
  - [Pre-Push Quality Checks](#pre-push-quality-checks)
  - [Building from Source](#building-from-source)
- [Documentation & References](#documentation--references)
- [License](#license)

---

## Why Heron?

Built for developers concurrently running multiple AI coding tools—such as **Claude Code, Codex, Cursor, Trae / Trae CN, Qoder, WorkBuddy, Cline, GitHub Copilot, Grok Build, Antigravity, opencode, and `.agents`**.

As Agent Skills (`SKILL.md` bundles containing prompts, execution workflows, scripts, and references) become core engineering assets, their storage and delivery models remain stuck in the stone age: manually copying and pasting folders into hidden system directories. This creates systematic friction:

| Problem Scenario | Traditional Workaround | What Heron Does |
|---|---|---|
| **Scattered**: Skills end up in various global and project directories | Hop across four or five hidden folders to inspect files | `heron scan` inspects everything at once, cleanly distinguishing "managed" vs "unowned" |
| **Duplicated**: The same skill is copied into multiple locations | Edit one copy, and the rest silently drift and become stale | `heron adopt` consolidates copies into **one single entity**, replacing originals with managed links |
| **Opaque Visibility**: No idea whether the client actually loads it | Guesswork debugging; often broken by symlink faults or name shadowing | `heron verify` strictly audits visibility: *visible / not scanned / shadowed / restart required / broken* |
| **Disconnected Upstream**: Authors push upstream updates unnoticed | Catching updates is purely accidental | `heron outdated` compares local commits against upstream commits, flagging pending updates |
| **Overwritten Customizations**: Upgrades wipe out local prompt tuning | Re-apply personal tweaks and patches after every upgrade | **Three-way merge (`git merge-tree`)**: Preserves upstream improvements while keeping your local customizations intact |
| **Team Drift**: Teammates work with mismatched skill sets | Ad-hoc chat messaging; new hires lack the required skill assets | `heron.lock` exports a portable lockfile; one command reproduces identical setups |
| **Fragmented Catalogs**: Siloed marketplaces and install tools | Manual downloads, zip extraction, or single-point shell scripts | `heron install` + unified search across **6 built-in marketplaces**, favourites, and any Git repo |

---

## Core Workflow: Adopt · Project · Upgrade

Heron collapses complex skill lifecycle management into a clean, closed loop:

```mermaid
flowchart LR
    A["Existing Scattered Skills<br/>(~/.claude, ~/.agents, project dirs, etc.)"] -->|"heron scan & adopt<br/>(Backup, Deduplicate, Register)"| B[("Heron Central Store<br/>~/.heron/skills/<br/>(Each Skill is an isolated Git repo)")]
    B -->|"heron project<br/>(Unidirectional Projection)"| C["Client & Project Targets<br/>(Claude / Cursor / Trae / Copilot, etc.)"]
    D["Remote Upstream / Markets<br/>(GitHub, skills.sh, ClawHub, ModelScope, etc.)"] -->|"heron upgrade<br/>(git merge-tree Three-way Merge)"| B
```

1. **Adopt**: Scans the disk for existing skills, creates safe backups, moves physical files into the central store (`~/.heron/skills/`), deduplicates identical content by hash, and replaces original locations with managed links (or keeps/live-binds in place);
2. **Project**: Unidirectionally projects skills from the store to explicitly selected client paths or workspace projects with per-target toggles;
3. **Upgrade**: Checks upstream releases and executes three-way merges in an isolated git tree. If conflicts occur, an interactive visual conflict resolver is surfaced, never dirtying your working tree.

---

## Interface Showcase

> Screenshots automatically adapt to your GitHub Light / Dark theme. The desktop app ships with **4 curated themes** (*Default*, *Dusk*, *Forest Ink*, *Evening Clear*), Auto / Light / Dark modes, and a global `⌘K` / `Ctrl+K` search palette.

| My Skills (Grid / List & Batch Projection) | Per-Client Projection Drawer |
|---|---|
| <picture><source media="(prefers-color-scheme: dark)" srcset="images/我的Skill-深色.png" /><source media="(prefers-color-scheme: light)" srcset="images/我的Skill-浅色.png" /><img alt="My Skills" src="images/我的Skill-浅色.png" /></picture> | <picture><source media="(prefers-color-scheme: dark)" srcset="images/我的Skill-深色2.png" /><source media="(prefers-color-scheme: light)" srcset="images/我的Skill-浅色2.png" /><img alt="Skill Projection Drawer" src="images/我的Skill-浅色2.png" /></picture> |

| Discover (Unmanaged Skills on Disk) | Sources & Markets (Unified Search) |
|---|---|
| <picture><source media="(prefers-color-scheme: dark)" srcset="images/发现-深色.png" /><source media="(prefers-color-scheme: light)" srcset="images/发现-浅色.png" /><img alt="Discover" src="images/发现-浅色.png" /></picture> | <picture><source media="(prefers-color-scheme: dark)" srcset="images/来源与市场-深色.png" /><source media="(prefers-color-scheme: light)" srcset="images/来源与市场-浅色.png" /><img alt="Sources and Markets" src="images/来源与市场-浅色.png" /></picture> |

<details>
<summary><strong>More views: Favourites, Updates, Health (Doctor), and Settings</strong></summary>

| My Favourites | Updates (Three-Way Merge) |
|---|---|
| <picture><source media="(prefers-color-scheme: dark)" srcset="images/我的收藏-深色.png" /><source media="(prefers-color-scheme: light)" srcset="images/我的收藏-浅色.png" /><img alt="My Favourites" src="images/我的收藏-浅色.png" /></picture> | <picture><source media="(prefers-color-scheme: dark)" srcset="images/更新-深色.png" /><source media="(prefers-color-scheme: light)" srcset="images/更新-浅色.png" /><img alt="Updates" src="images/更新-浅色.png" /></picture> |

| Health / Doctor (Read-Only Diagnostics) | Settings (4 Themes, Modes & General) |
|---|---|
| <picture><source media="(prefers-color-scheme: dark)" srcset="images/体检-深色.png" /><source media="(prefers-color-scheme: light)" srcset="images/体检-浅色.png" /><img alt="Health Diagnostics" src="images/体检-浅色.png" /></picture> | <picture><source media="(prefers-color-scheme: dark)" srcset="images/设置-深色.png" /><source media="(prefers-color-scheme: light)" srcset="images/设置-浅色.png" /><img alt="Settings" src="images/设置-浅色.png" /></picture> |

</details>

---

## System Architecture & Workspace Crates

Heron follows a strict **unidirectional dependency layered architecture**. The business service layer orchestrates all use cases, while the GUI and CLI share the exact same service core to guarantee 100% behavioral parity:

```mermaid
graph TD
    subgraph UI_Layer ["Presentation Layer"]
        UI["Vue 3 + TypeScript GUI<br/>(ui/)"] <-->|"Tauri 2 IPC / Events"| Shell["Tauri Desktop Shell<br/>(src-tauri)"]
        CLI["heron CLI<br/>(crates/heron-cli)"]
    end

    subgraph Service_Layer ["Service Orchestration Layer"]
        Shell --> Service["heron-service<br/>(Plan -> Preview / dry-run -> Apply with Undo Audits)"]
        CLI --> Service
    end

    subgraph Core_Engines ["Core Engines"]
        Service --> Scan["heron-scan<br/>(Read-only Traversal, Frontmatter, Hashing)"]
        Service --> Proj["heron-projection<br/>(Projection Planning, Token Budget, Visibility)"]
        Service --> VCS["heron-vcs<br/>(Per-Skill Git, Three-way Merge)"]
        Service --> Git["heron-git<br/>(Remote Bare Cache, Subdir Checkouts)"]
        Service --> Source["heron-source<br/>(Market Adapters, Git URL Parsing)"]
        Service --> Store["heron-store<br/>(SQLite Metadata, Migrations, Audits)"]
    end

    subgraph Foundation ["Infrastructure & Domain Foundation"]
        Service --> Plat["heron-platform<br/>(Link Safety Contract, Atomic IO, Probe)"]
        Scan --> Core["heron-core<br/>(Pure Domain Models, No IO, Structured Msg)"]
        Proj --> Core
        VCS --> Core
        Store --> Core
    end
```

### Workspace Crate Responsibility Matrix

| Crate | Responsibilities & Technical Highlights | Path |
|---|---|---|
| **`heron-core`** | **Pure Domain Model**: Strictly no IO, async, or system clock. Defines domain models (`Slug`, `Hash`, `PlatformKind`), policies, and localized structured error codes (`Msg`). | [`crates/heron-core`](crates/heron-core) |
| **`heron-platform`** | **OS Abstraction**: Link safety contract (`link`), atomic file mutations, cross-platform locking, process probing (deadline-bound `which` fixing Finder's stripped `PATH`), and automatic proxy detection. | [`crates/heron-platform`](crates/heron-platform) |
| **`heron-store`** | **Metadata Persistence**: Embedded SQLite via `rusqlite` (bundled). Manages clients, targets, skills, projections, source bindings, favourites, and audit events. | [`crates/heron-store`](crates/heron-store) |
| **`heron-scan`** | **Read-Only Scanner**: Multi-threaded disk traversal (`rayon` + `walkdir`), YAML frontmatter parser, Blake3 fingerprinting, streaming progress events, and cooperative cancellation. | [`crates/heron-scan`](crates/heron-scan) |
| **`heron-vcs`** | **Per-Skill Version Control**: Manages independent Git repositories for every skill. Executes in-memory `git merge-tree` three-way merges without touching the working tree. | [`crates/heron-vcs`](crates/heron-vcs) |
| **`heron-git`** | **Remote Git Engine**: Shallow fetching, branch/tag resolution, and local bare Git caching (`sources-cache`). | [`crates/heron-git`](crates/heron-git) |
| **`heron-source`** | **Marketplace & Source Adapters**: Adapters for 6 built-in marketplaces (`skills.sh`, `ClawHub`, `Qoder`, `SkillHub`, `ModelScope`, `SkillsMP`); parses complex Git URLs (repositories, monorepo subpaths, blob links, SSH, local repos). | [`crates/heron-source`](crates/heron-source) |
| **`heron-projection`** | **Projection & Verification**: Computes projection plans, estimates token budgets, executes safe filesystem linking, and audits live client loading visibility. | [`crates/heron-projection`](crates/heron-projection) |
| **`heron-service`** | **Use Case Orchestrator**: High-level business logic (adopt, project, upgrade, doctor, install, scan, lock). Enforces `Plan -> Preview (dry-run) -> Apply` with backup safety. | [`crates/heron-service`](crates/heron-service) |
| **`heron-cli`** | **Unified CLI**: Built on `clap` 4, supporting formatted terminal output and `--json` machine-readable output. | [`crates/heron-cli`](crates/heron-cli) |
| **`src-tauri`** | **Tauri 2 Shell**: Window management, single-instance guard, system tray, autostart, minisign-verified updater, IPC routing, and background task event streaming (zero business logic). | [`src-tauri`](src-tauri) |
| **`ui`** | **Frontend Application**: Vue 3 + TypeScript + Vite + Pinia + Vue Router + Vue I18n thin client with custom design tokens and 4 built-in themes. | [`ui`](ui) |

---

## Core Mechanisms & Technical Highlights

### 1. Single Source of Truth & 4 Projection Modes

The store (`~/.heron/skills/<slug>`) is the single source of truth. To ensure that different AI clients and operating systems can reliably read skills, Heron supports **4 projection modes**:

1. **`symlink` (Symbolic Link)**: Default for macOS and Linux. Zero disk duplication with instant synchronization;
2. **`junction` (Directory Junction)**: Default on Windows. Provides cross-directory links **without requiring Administrator privileges** or Developer Mode;
3. **`pointer` (Pointer File)**: Emits a lightweight proxy `SKILL.md` pointing to the real store path, solving compatibility bugs where certain client scanners only read `.md` files or fail on symlinked directories;
4. **`copy` (Unidirectional Copy)**: Fallback mode equipped with drift detection (`E_DRIFTED`), refusing to overwrite unexpected external edits.

### 2. Per-Skill Git Repositories & Three-Way Merge Upgrades

Each managed skill in the store is initialized as an isolated Git repository:
* **`main` branch**: The user's active working branch (all local custom prompt tweaks are committed here);
* **`upstream` branch**: The mirror of upstream releases (written only during Heron upgrade cycles);
* **`v<version>` tags**: Historical release points allowing one-click rollbacks via `heron rollback` (which also records a `heron-rollback-backup-*` tag);
* **Three-Way Merge**: Upgrades invoke `git merge-tree --write-tree --merge-base=<baseline>`:
  * **Auto-Merge**: When local changes and upstream improvements do not overlap, they merge cleanly without losing custom lines;
  * **Conflict Protection**: If conflicts occur, the working tree remains untouched (exit code `3`). Conflict payloads are parsed and displayed in a side-by-side per-file merge editor in the desktop app, or resolved via `heron resolve`;
  * **Overwrite Option**: Prefer taking upstream wholesale? Pass `--overwrite` in CLI or toggle *"Keep local edits on upgrade"* in Settings.

### 3. Dynamic Presence Detection for 13 AI Clients

Out-of-the-box definitions in [`resources/clients/`](resources/clients) for **13** AI coding tools, each annotated with `verified_by` (`spec` / `doc` / `probe`):
* **Supported Clients**: Claude Code, Codex, Cursor, Trae, Trae CN, Qoder, WorkBuddy, Cline, GitHub Copilot, Grok Build, Antigravity, opencode, and Agents (`.agents`);
* **Non-Invasive Probing**: Probes installation status via compound signals (standard configuration directories, CLI commands, and macOS `.app` bundles);
* **Orthogonal State**: Undetected clients are greyed out with an honest "Not detected on this machine" badge, **never deleting configurations and never modifying user-defined `enabled` switches**.

### 4. Unified Search Across 6 Built-in Marketplaces & Favourites

Heron integrates **6** skill marketplaces with honest capability grading:
* **Indexed (Searchable in-app)**: `skills.sh` (resolves straight to GitHub repos) and `ClawHub` (REST API search; installs via Git when an upstream repo URL is published, otherwise opens guided page);
* **Guided (Browser deep-link + paste URL to install)**: `Qoder`, `SkillHub`, `ModelScope` (pasted ModelScope skill page URLs resolve directly to a Git clone), and `SkillsMP` (disabled by default);
* **My Favourites**: Bookmark repositories or market entries from anywhere, preview install plans, and install or reinstall with one click.

### 5. Team Collaboration & Lockfile

Similar to modern package managers, Heron supports exporting a portable `heron.lock` (recording sources, commits, versions, and content hashes—never absolute paths):

```bash
heron lock export                       # Write heron.lock in the current directory (or --dir <DIR> / --print)
heron lock diff --file heron.lock       # Detect Missing / ContentChanged / SourceChanged drift
heron lock apply --file heron.lock      # Install missing skills to reproduce the team setup
```

---

## Quick Start

### Requirements

- **git 2.38+** (Required: Three-way merges depend on `git merge-tree --write-tree`);
- **macOS 12+** or **Windows 10 1803+** (Windows uses the built-in WebView2 runtime);
- Linux core is functional; official release packages currently target macOS (Apple Silicon) and Windows (x64).

### Install

* **Desktop Application (Recommended)**: Download `Heron_0.1.0_aarch64.dmg` (macOS Apple Silicon) or `Heron_0.1.0_x64-setup.exe` (Windows x64) from [Releases](https://github.com/v5note/heron/releases).
  * *First-launch note*: Releases use Tauri's **minisign update signing** without paid OS code-signing certificates. On first launch, right-click → Open on macOS, or click *"More info" → "Run anyway"* on Windows. Subsequent in-app updates are cryptographically verified via minisign. See [`docs/SIGNING.md`](docs/SIGNING.md) for details.
* **Command Line**: Build from source or use the standalone `heron` binary:
  ```bash
  heron --version
  heron --help
  ```

### Three Commands to Get Going

```bash
# 1. Scan known directories and build a global asset index
heron scan

# 2. Inspect skills found on disk that are not managed yet
heron detected

# 3. Adopt all unmanaged skills into the central store (backs up first, deduplicates)
heron adopt --all
```

> [!TIP]
> Before running adopt for the first time, use `--dry-run` to preview all planned mutations:
> `heron adopt --all --dry-run`.

After adoption, original directories remain intact and become links pointing back to the central store:
```text
Before Adoption                               After Adoption
~/.agents/skills/wechat-mp/          →             ~/.agents/skills/wechat-mp   ──┐ (Symlink / Junction)
~/.claude/skills/wechat-mp/          →             ~/.claude/skills/wechat-mp   ──┼─→ ~/.heron/skills/wechat-mp (Single Source of Truth)
/path/to/project/.cursor/wechat-mp/  →             .../project/.cursor/wechat-mp ─┘
```

---

## Desktop App Navigation

The desktop application follows a **Workbench → Collect · Distribute → Maintain** layout with a global search palette (`⌘K` / `Ctrl+K`), native menu bar (macOS/Linux), system tray, and instant theme/language switching:

| Group | View | Purpose |
|---|---|---|
| **Workbench** | **Overview (`概览`)** | **Action Center**: Decision queue showing only items needing action (broken links, merge conflicts, available upgrades, unmanaged skills), live system readouts, and one-click client target switches. |
| **Collect · Distribute** | **My skills (`我的 Skill`)** | **Asset Library**: List & Grid views with batch projection (`Project to...`), status filtering (`All` / `Issues`), and a detail drawer for per-client enable/disable toggling. |
| | **Discover (`发现`)** | Browse unmanaged skills detected on disk; filter by client, choose placement strategy (*recommended / replace / keep-original / live-bind*), preview, and batch adopt. |
| | **My favourites (`我的收藏`)** | Bookmark Git repositories and marketplace links; filter by installable vs link-only vs installed, preview plans, and install with one click. |
| | **Sources & markets (`来源与市场`)** | Unified search across 6 built-in marketplaces with live latency probing, grouped results, direct Git URL installs, and one-click bookmarking. |
| **Maintain** | **Updates (`更新`)** | Tracks upstream commits vs local edits (`up to date`, `has update`, `local modified`, `diverged`, `not tracked`); runs three-way merges with a visual per-file conflict editor. |
| | **Health (`体检`)** | Read-only diagnostics for broken links, missing store directories, duplicate content, and pathological nesting (`skills/.claude/skills/`), with explicit confirmation dialogs for any cleanup. |
| | **Projects (`项目`)** | Discover and register workspace projects containing client skill directories (`.claude/skills`, `.cursor/skills`, etc.). |
| | **Settings (`设置`)** | 4 color themes (*Default*, *Dusk*, *Forest Ink*, *Evening Clear*), Auto/Light/Dark mode, language (English / Simplified Chinese), default projection mode, auto-scan/auto-doctor toggles, merge preference, proxy, autostart, minisign-verified app updates, and client capability table. |

---

## CLI Command Overview

All commands support `--json` (for CI pipelines and scripting) and `--data-dir <DIR>` (for isolated execution). Exit codes: `0` success, `1` error, `2` usage error, `3` user decision required (e.g., merge conflict).

| Command | Purpose |
|---|---|
| `heron scan [--roots <PATH>...] [--max-dirs <N>] [--verbose]` | Scan known client paths and optional project roots to refresh the index |
| `heron detected [--state <STATE>] [--all]` | List unmanaged skills found on disk (`candidate\|ignored\|adopted\|conflict\|all`) |
| `heron adopt <SLUG\|PATH...> \| --all` | Safely adopt skills into the store (`--placement replace\|keep-original\|live-bind`, `--keep-original`, `--dry-run`) |
| `heron list` | List all managed skills and their origin |
| `heron targets` | List all known projection targets |
| `heron project <SLUG> --on <TARGET> [--mode ...]` | Project a skill to a target (`symlink\|junction\|copy\|pointer`; `--on` is required) |
| `heron unproject <SLUG> --on <TARGET>` | Retract projection from a target (removes link, preserves store copy) |
| `heron verify [<SLUG>]` | Audit whether clients will actually load projected skills |
| `heron doctor` | Run non-destructive system diagnostics |
| `heron install <INPUT> [--ref ...] [--subpath ...] [--as ...]` | Install skills from GitHub/GitLab, monorepo subpaths, SSH URLs, or local Git repos |
| `heron outdated` | Check managed skills against upstream commits |
| `heron upgrade [<SLUG>...] [--all] [--overwrite] [--dry-run]` | Upgrade skills via three-way merge (or overwrite local edits with `--overwrite`) |
| `heron rollback <SLUG> --to <VERSION\|TAG>` | Roll back a skill to a specific release tag |
| `heron diff <SLUG> [--against <TAG>]` | View local modifications (including uncommitted changes) or diff against a tag |
| `heron conflict <SLUG>` | Inspect conflicting files and both sides of a pending upgrade |
| `heron resolve <SLUG> [--take ours\|theirs]` | Resolve conflict files and retry the upgrade |
| `heron market list \| search \| plan` | List, search, and preview install plans across the 6 built-in marketplaces |
| `heron lock export \| diff \| apply` | Export (`--dir`/`--print`), diff (`--file`), and apply (`--file`) team lockfiles |
| `heron projects [--roots <PATH>...] [--register] [--depth <N>]` | Discover and optionally register workspace projects containing client skill directories |

For complete command syntax, flags, and error codes, see the [Command & Error Reference (docs/REFERENCE.md)](docs/REFERENCE.md).

---

## Design Principles & Safety Contracts

> **Heron's Golden Rule: Never silently destroy user data.**

- **Safety Net**: Before mutating or moving existing directories, Heron creates a full backup under `<data-dir>/backups/`; if backup creation fails, the transaction aborts immediately;
- **No Overwrite of Foreign Content**: Projections stop with `E_NOT_MANAGED` when encountering an unmanaged non-empty directory;
- **Drift Protection**: Under `copy` mode, if a target file was modified externally, Heron refuses to overwrite it and reports `E_DRIFTED`;
- **Non-Destructive Doctor**: Diagnostic issues like pathological nesting (`skills/.claude/skills/...`) or missing store directories are reported with recommended remedies—cleanup always requires explicit user confirmation;
- **Honesty in Capabilities**:
  - Markets are categorized explicitly: *Indexed* (searchable in-app) vs *Guided* (browser navigation);
  - Uninstalled client environments are labelled as undetected rather than claiming phantom "active" links;
  - Locally adopted skills without remote tracking are labelled `not tracked` rather than falsely showing as "up-to-date".

---

## Where Data Lives & Storage Isolation

Heron strictly separates the **Single Source of Truth (Store)** from **Runtime Data (Data Directory)**:

| Platform | Runtime Data Directory (`heron.db`, `backups/`, `sources-cache/`, `trash/`, `log/`) | Central Store (Each skill's Git repo) |
|---|---|---|
| **macOS** | `~/Library/Application Support/xin.v5ai.heron/` | `~/.heron/skills/` |
| **Windows** | `%LOCALAPPDATA%\v5ai\heron\data\` | `%USERPROFILE%\.heron\skills\` |
| **Linux** | `~/.local/share/heron/` | `~/.heron/skills/` |

> [!NOTE]
> - `~/.agents/skills` is merely an optional projection target (shared mapping location). It is **not** the Heron store. Heron only manages entries that it created.
> - Passing `--data-dir <DIR>` relocates **everything** Heron writes—including config, cache, logs, backups, `heron.db`, and the store (`<DIR>/.heron/skills/`)—so tests and portable setups never touch your real `~/.heron`.

---

## Development & Contributing

### Prerequisites

- **Rust**: Stable (pinned via `rust-toolchain.toml`, managed with `rustup`);
- **Node.js**: v22+, with **pnpm** 10+ (`corepack enable`);
- **Git**: 2.38+;
- **Linux Build Dependencies** (not required on macOS / Windows):
  ```bash
  sudo apt install libwebkit2gtk-4.1-dev libgtk-3-dev libayatana-appindicator3-dev librsvg2-dev
  ```

### One-Time Setup

```bash
# 1. Install frontend dependencies
pnpm --dir ui install

# 2. Compile Rust workspace
cargo build --workspace
```

> **Note (Read-only HOME / Sandboxed Environments):** If `~/.cargo` is read-only, point Cargo to a local directory: `export CARGO_HOME=$PWD/.cargo-home`.

### Local Development

```bash
# Start full desktop app (Tauri hot reload, occupies port 5183)
scripts/dev.sh dev

# No tauri-cli? the command still works: it starts Vite and runs
# `cargo run -p heron-app` itself, so Rust edits need a manual re-run.

# Port 5183 held by a leftover dev server? let the script reap it
KILL_STALE_DEV=1 scripts/dev.sh dev

# Frontend-only dev server (Browser opens http://localhost:5183, no Rust IPC)
scripts/dev.sh web

# Run CLI against an isolated test environment (redirects data to /tmp/heron-dev)
scripts/dev.sh cli scan
scripts/dev.sh cli list
```

Windows exposes the same entry points through `scripts\dev.ps1`, wrapped by `scripts\dev.cmd` (it bypasses the default execution policy and prefers PowerShell 7 when installed). The commands below are written for `cmd.exe`; in PowerShell prefix them with `.\`, e.g. `.\scripts\dev.cmd dev`:

```powershell
# Full desktop app (Tauri hot reload, occupies port 5183)
scripts\dev.cmd dev

# No tauri-cli? the command still works: it starts Vite and runs
# `cargo run -p heron-app` itself, so Rust edits need a manual re-run.

# Frontend-only dev server (no Rust IPC)
scripts\dev.cmd web

# Run CLI against an isolated data dir (defaults to %TEMP%\heron-dev)
scripts\dev.cmd cli scan
scripts\dev.cmd cli list

# Isolate the data dir yourself: set HERON_DEV_DATA before running the CLI
$env:HERON_DEV_DATA = 'D:\heron-dev'; .\scripts\dev.cmd cli scan

# Port 5183 held by a leftover dev server? let the script reap it
#   cmd:        set KILL_STALE_DEV=1 && scripts\dev.cmd dev
#   PowerShell: $env:KILL_STALE_DEV = '1'; .\scripts\dev.cmd dev
```

#### Protecting Your Personal Skills During Dev

```bash
# 1. Isolate the data directory (database, store, and backups redirect together)
./target/debug/heron --data-dir /tmp/heron-dev scan

# 2. Isolate the HOME directory visible to the scanner
HOME=/tmp/fake-home ./target/debug/heron --data-dir /tmp/heron-dev scan
```

### Debugging Cheat Sheet

| Task | Command / Technique |
|---|---|
| **Inspect Backend Logs** | `RUST_LOG=debug cargo run -p heron-app` |
| **Open WebView DevTools** | Right-click in dev window → "Inspect", or press `Cmd+Opt+I` / `Ctrl+Shift+I` |
| **Query Index SQLite Database** | `sqlite3 <data-dir>/heron.db "select * from skill;"` |
| **Inspect Skill Git History** | `git -C ~/.heron/skills/<slug> log --graph --oneline` |
| **Diagnose Inactive Projections** | `heron verify` (inspect not-scanned, shadowed, or broken link causes) |
| **Inspect Upgrade Conflicts** | `heron conflict <slug> --json` |
| **Run Specific Integration Test** | `cargo test -p heron-service --test adopt_flow -- --nocapture` |

### Pre-Push Quality Checks

```bash
scripts/dev.sh check    # fmt + clippy (matches CI rules)
scripts/dev.sh test     # run all unit and integration tests
scripts/dev.sh ui       # TypeScript typecheck, i18n parity, IPC check & build
```

> Windows: `scripts\dev.cmd check` / `test` / `ui` run the identical checks.

### Building from Source

```bash
pnpm --dir ui build
cargo build -p heron-cli --release   # CLI binary → target/release/heron
cargo build -p heron-app --release   # Desktop binary
scripts/dev.sh bundle                # Native installer packages (.dmg / NSIS installer)
```

> Windows: `scripts\dev.cmd bundle` produces the NSIS installer (`.exe`); it needs `tauri-cli` (`cargo install tauri-cli --version "^2" --locked`).

---

## Documentation & References

- **[Getting Started Guide (docs/GUIDE.md)](docs/GUIDE.md)** —— Step-by-step tutorial from chaos to order
- **[Command & Error Reference (docs/REFERENCE.md)](docs/REFERENCE.md)** —— Complete CLI manual and structured error codes
- **[Release & Signing (docs/SIGNING.md)](docs/SIGNING.md)** —— Minisign update signing, Gatekeeper/SmartScreen notes, and release workflow
- **[Design Document v2 (docs/DESIGN-heron-v2.md)](docs/DESIGN-heron-v2.md)** —— Architectural decisions and background rationale
- **[Implementation Document v1 (docs/IMPLEMENTATION-heron-v1.md)](docs/IMPLEMENTATION-heron-v1.md)** —— Component decomposition and milestones
- **[Architecture Decision Records (docs/adr/)](docs/adr/)** —— ADR records for platform edge cases and design trade-offs

---

## License

This project is licensed under the [MIT License](LICENSE).
