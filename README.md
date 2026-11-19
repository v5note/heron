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
~/.agents/skills/pdf/     →     ~/.agents/skills/pdf → <data>/store/pdf
~/.claude/skills/pdf/     →     ~/.claude/skills/pdf → <data>/store/pdf   (duplicate, deduplicated)
```

> Always run `heron adopt --all --dry-run` first: it prints every step without touching the disk.

---

## Commands

| Command | Purpose |
|---|---|
| `heron scan` | Scan all known locations, refresh the index |
| `heron detected` | List skills Heron does not manage yet |
| `heron adopt <slug...> \| --all` | Move them into the store |
| `heron list` | Managed skills |
| `heron project <slug> [--on <target>]` | Materialize into a target directory |
| `heron unproject <slug> [--on <target>]` | Remove a projection (store copy is kept) |
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
| macOS | `~/Library/Application Support/io.greyheron.heron` |
| Windows | `%LOCALAPPDATA%\heron` |
| Linux | `~/.local/share/heron` |

`store/` is the single source of truth (one git repository per skill), `backups/` is the safety net,
`trash/` is a soft-delete area. The index (`heron.db`) can be rebuilt from the store at any time.

> `~/.agents/skills` is **not** Heron's store. Heron has its own default landing directory
> (`~/.heron/skills`); `.agents/skills` is only an optional shared drop point, because `npx skills`
> uses that directory too.

---

## Build from source

```bash
# Requires Rust stable, Node 22, pnpm 10
pnpm --dir ui install
pnpm --dir ui build
cargo build -p heron-cli --release   # CLI    → target/release/heron
cargo build -p heron-app --release   # desktop app
```

Local checks (same as CI):

```bash
scripts/dev.sh check   # fmt + clippy
scripts/dev.sh test    # all tests
scripts/dev.sh ui      # frontend typecheck + i18n parity + build
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
- [Decision records](docs/adr/) — the trade-offs made along the way

## License

MIT OR Apache-2.0
