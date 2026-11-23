# Heron

**Gather the Agent Skills scattered across your machine into one versioned copy, project them where your tools actually look, and make upgrading a single command.**

> English · [中文](README.zh-CN.md)

Heron is for developers running several AI coding tools at once. Skills end up in `~/.claude/skills`,
`~/.codex/skills`, `~/.agents/skills` and in the `.claude` directory of every project. Edit one copy and
the rest go stale; upgrade one and you lose your local edits. Heron collapses that into three verbs:
**adopt → project → upgrade**.

- macOS 12+ and Windows 10 1803+ (runs on Linux; not a first-class target yet)
- Desktop app **and** command line, backed by the same code — identical behaviour
- Every change can be previewed and rolled back; nothing is deleted before it is backed up

---

## What it solves

| Pain | What Heron does |
|---|---|
| Skills scattered across five hidden directories | `heron scan` finds everything; lists "managed" vs "unowned" |
| The same skill copied several times | `heron adopt` keeps **one** copy and turns the other locations into links |
| No idea whether the client actually loads it | `heron verify` distinguishes *live / not scanned / shadowed / broken* |
| Upstream moved and nobody noticed | `heron outdated` compares against the upstream commit |
| Upgrading overwrites my edits | Three-way merge keeps both your changes and upstream's |
| Every teammate has a different skill set | `heron.lock` exports and reproduces it |
| Installing skills from GitHub or a marketplace | `heron install` + five built-in marketplaces |

---

## Quick start

### Requirements

- **git 2.38+** (three-way merges need `git merge-tree --write-tree`). If git is missing, Heron tells
  you how to install it instead of silently degrading.
- macOS 12+ / Windows 10 1803+ (Windows needs WebView2, built into Windows 11).

### Install

**Desktop app** (recommended): download `Heron.dmg` (macOS) or `Heron_x64-setup.exe` (Windows).

**Command line**, shipped alongside the app and usable on its own:

```bash
heron --help
```

### Three commands to get going

```bash
heron scan        # look around
heron detected    # what Heron does not manage yet
heron adopt --all # take it all over (backs up first, de-duplicates)
```

Your original directories do not disappear — they become links into the store:

```text
before                          after
~/.agents/skills/pdf/     →     ~/.agents/skills/pdf → ~/.heron/skills/pdf
~/.claude/skills/pdf/     →     ~/.claude/skills/pdf → ~/.heron/skills/pdf   (duplicate, deduplicated)
```

> Always run `heron adopt --all --dry-run` first: it prints every step without touching the disk.

### 4. Using the desktop app

The app is organised as **collect → distribute → maintain**. It opens in **dark**; switch to light from the
button at the bottom of the rail, or from Settings.

| Where | What it is for |
|---|---|
| **Overview** | Only what needs deciding *now*: broken links, files to merge, upgrades, unmanaged skills. Otherwise a single "Nothing needs you right now" |
| **My skills** | One row per skill, one column per client: filled dot = live, amber = shadowed, red = broken link, hollow = not scanned. Open a row for the detail drawer and project/unproject each target |
| **Discover** | Skills on disk that Heron does not manage yet; selection shows a read-only dry run before anything moves |
| **Sources & markets** | Five markets: two are searchable and installable, three are browse-only (paste the git URL back) |
| **Updates** | Local changes, upstream updates and diverged skills are listed apart; conflicts open a file-by-file merge and retry automatically |
| **Health** | Read-only checks, grouped by error/warning; nothing on disk is touched |
| **Projects** | The directories clients actually read (targets) plus project targets, each switchable |
| **Settings** | Theme (dark/light), language, app updates, client native-scan capability |

---

## Commands

| Command | Purpose |
|---|---|
| `heron scan` | Scan all known locations, refresh the index |
| `heron detected` | List skills Heron does not manage yet |
| `heron adopt <slug...> \| --all` | Move them into the store |
| `heron list` | Managed skills |
| `heron project <slug> --on <target>` | Materialize into a target directory (no default target — name it) |
| `heron unproject <slug> --on <target>` | Remove a projection (store copy is kept) |
| `heron verify` | Would a client actually load this skill? |
| `heron doctor` | Read-only health check: duplicates, broken links, nested trees, lost sources |
| `heron install <url>` | Install from a git repository |
| `heron outdated` / `upgrade` / `rollback` | Check / three-way-merge upgrade / roll back |
| `heron diff` / `conflict` / `resolve` | Inspect changes / inspect conflicts / resolve them |
| `heron market list\|search\|plan` | Five built-in marketplaces |
| `heron lock export\|diff\|apply` | Team workflow |
| `heron projects` | Discover projects that contain client directories |

Every command accepts `--json` (for scripts) and `--data-dir` (an isolated data directory).

Full details: **[getting-started guide](docs/GUIDE.md)** · **[command reference](docs/REFERENCE.md)**.

---

## Design choices worth knowing

**Heron never damages anything on its own.** These behaviours are deliberate, not gaps:

- Adoption always backs up first; if the backup fails, the whole operation aborts.
- Projection stops with `E_NOT_MANAGED` when someone else's real directory is in the way.
- In `copy` mode, a destination changed by another program reports `E_DRIFTED` instead of being overwritten.
- Pathological nesting (`skills/.claude/skills/…`) is **reported, never reorganised**.
- Heron never executes scripts from a skill.

**Heron does not pretend.** Marketplaces come in two classes: *searchable* (skills.sh and ClawHub,
which Heron can install from) and *browser only* (qoder, SkillHub, SkillsMP — Heron opens the site and
you paste a git URL back). Skills adopted from local disk show up as `not tracked` with version
`0.0.0`; they are not presented as upgradable. Client native-scan injection is off by default and
never writes to a client that has not been verified.

---

## Where data lives

| Platform | Data directory |
|---|---|
| macOS | `~/Library/Application Support/xin.v5ai.heron` |
| Windows | `%LOCALAPPDATA%\v5ai\heron\data` |
| Linux | `~/.local/share/heron` |

`~/.heron/skills` is the store — the single source of truth, one git repository per skill. It is not
inside the data directory. The data directory holds `backups/` (the safety net), `trash/`
(a soft-delete area) and `heron.db`, which can be rebuilt from the store at any time.

> `~/.agents/skills` is a projection target, not the store — `npx skills` writes there too, so Heron
> only ever touches the entries it created itself.

---

## Development

### Prerequisites

| Tool | Version | Notes |
|---|---|---|
| Rust | stable | `rust-toolchain.toml` pins it; rustup picks it up automatically |
| Node.js | 22+ | needed for the frontend |
| pnpm | 10+ | `corepack enable` installs it |
| git | 2.38+ | three-way merges need `git merge-tree --write-tree` |

On Linux, Tauri also needs system libraries:

```bash
sudo apt install libwebkit2gtk-4.1-dev libgtk-3-dev libayatana-appindicator3-dev librsvg2-dev
```

### One-time setup

```bash
pnpm --dir ui install     # frontend dependencies
cargo build --workspace   # first Rust build (several minutes)
```

> **Sandboxed / air-gapped machines:** every Cargo command in this project expects
> `CARGO_HOME` to point at a writable directory. If `~/.cargo` is not writable, export
> `CARGO_HOME=$PWD/.cargo-home` before building — `.cargo-home/` is already git-ignored.

### Running it

```bash
scripts/dev.sh dev      # desktop app with hot reload (needs tauri-cli)
scripts/dev.sh web      # frontend only, in a browser, no Rust backend
scripts/dev.sh cli …    # the CLI against an isolated data dir
```

**Full desktop app, one process** (hot reload for both halves):

```bash
cargo install tauri-cli --version '^2' --locked   # once
scripts/dev.sh dev        # equivalent to: cargo tauri dev
```

Tauri starts Vite on port **5183** (strict, so a port clash fails loudly) and rebuilds Rust on
change. The app loads `http://localhost:5183`.

**Two processes, no tauri-cli** — this is what `scripts/dev.sh dev` tells you to do when
tauri-cli is missing, and it is also the easiest way to debug the frontend alone:

```bash
# terminal 1 — frontend with hot reload only
pnpm --dir ui dev
# terminal 2 — Rust shell
cargo run -p heron-app
```

**Frontend only, no Rust:**

```bash
scripts/dev.sh web        # open http://localhost:5183 in any browser
```

Tauri commands will reject with an error there (there is no Rust side). That is expected;
it is the fastest loop for layout and styling work.

**CLI:**

```bash
scripts/dev.sh cli scan                              # isolated data dir under /tmp/heron-dev
HERON_DEV_DATA=/tmp/mine scripts/dev.sh cli list     # choose the isolated dir
cargo run -p heron-cli -- --data-dir /tmp/x scan     # plain cargo, no wrapper
```

### Isolating your real skills

Development must never touch `~/.claude` or `~/.codex`. Two levers:

```bash
# 1. isolate the data directory (index, store, backups all move together)
./target/debug/heron --data-dir /tmp/heron-dev scan

# 2. isolate the home directory seen by the scanner
HOME=/tmp/fake-home ./target/debug/heron --data-dir /tmp/heron-dev scan
```

`--data-dir` redirects data, config, cache, logs **and the store** (`<data-dir>/.heron/skills`), so
nothing lands in system paths and the real `~/.heron` is never written to.
Build a fixture tree under `/tmp/fake-home` (`~/.claude/skills/foo/SKILL.md`, …) to exercise
adoption without risking your own configuration.

### Debugging

| What | How |
|---|---|
| Rust logs | `RUST_LOG=debug cargo run -p heron-app`; CLI uses `--verbose` where available |
| WebView devtools | right-click → Inspect (dev builds), or `cmd+opt+I` / `ctrl+shift+I` |
| Frontend network/RPC | devtools console; every Tauri call is one `invoke`, see `ui/src/api/index.ts` |
| Frontend errors | `useAppStore().toasts` — every failed command surfaces as a localised toast |
| Inspect the index | `sqlite3 <data>/heron.db '.tables'` and `select * from skill;` |
| Inspect a skill's history | `git -C ~/.heron/skills/<slug> log --oneline` |
| Why a skill is invisible | `heron verify` — distinguishes *not scanned* from *shadowed* from *broken* |
| Why an upgrade conflicted | `heron conflict <slug>` (add `--json` for full file contents) |
| Backend panic | `RUST_BACKTRACE=1 cargo run -p heron-app` |

A single Rust test with output:

```bash
cargo test -p heron-service --test remote_flow -- --nocapture
cargo test -p heron-platform --test link_safety_windows -- --nocapture   # Windows only
```

Live marketplace checks are `#[ignore]`d so CI stays offline-safe:

```bash
cargo test -p heron-source --test live_markets -- --ignored --nocapture
```

### Before you push

```bash
scripts/dev.sh check    # fmt + clippy, exactly as CI runs it
scripts/dev.sh test     # all tests
scripts/dev.sh ui       # typecheck + i18n parity + build
```

Frontend strings live only in `ui/src/i18n/locales/{zh-CN,en-US}.json` and must stay in sync —
`scripts/dev.sh ui` fails if a key exists in one file but not the other. Rust never emits
user-facing prose; it returns `Msg { code, params }` and the UI renders `errors.<code>`.

### Locking down the toolchain

`rust-toolchain.toml` pins the Rust version, so every contributor and CI builds with the same
compiler. If `~/.cargo` is read-only, point `CARGO_HOME` somewhere writable (see above) rather
than editing the pin.

## Build from source

```bash
pnpm --dir ui build
cargo build -p heron-cli --release   # CLI    → target/release/heron
cargo build -p heron-app --release   # desktop app
scripts/dev.sh bundle                # dmg / NSIS installer (needs tauri-cli)
```

## Layout

```text
crates/
  heron-core        domain model (no IO)
  heron-platform    the only platform-specific layer: links, locks, paths, dirs
  heron-store       SQLite index and migrations
  heron-scan        traversal, frontmatter parsing, content fingerprints
  heron-vcs         one git repository per skill, three-way merges
  heron-git         bare cache and subdirectory checkouts
  heron-source      URL parsing, catalogs, marketplace adapters
  heron-projection  projection planning / execution / visibility
  heron-service     use cases shared by the CLI and the GUI
  heron-cli         command line
src-tauri/          Tauri 2 shell (thin adapter)
ui/                 Vue 3 frontend
docs/               design, implementation, guides and decision records
```

## Documentation

- [Design](docs/DESIGN-heron-v2.md) — why it works this way
- [Implementation](docs/IMPLEMENTATION-heron-v1.md) — how it is built, in what order
- [Getting started](docs/GUIDE.md) — from a messy machine to a managed set
- [Command reference](docs/REFERENCE.md) — every command, every error code
- [Signing & releases](docs/SIGNING.md) — how releases are signed without paid certificates
- [Decision records](docs/adr/) — the trade-offs made along the way

## License

MIT. See [LICENSE](LICENSE) for details.
