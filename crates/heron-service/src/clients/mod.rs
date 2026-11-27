//! The client matrix, loaded from `resources/clients/*.toml` (design §7.2).
//!
//! The files ship inside the binary and are the only place client paths are written down. Each
//! entry states how it was verified, so the UI can be honest about unverified assumptions.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use heron_store::db::Db;
use heron_store::models::{ClientRow, TargetRow};
use heron_store::repos::{client, projection, target};
use serde::Deserialize;

use crate::Result;

const BUILTIN_FILES: &[(&str, &str)] = &[
    (
        "agents",
        include_str!("../../../../resources/clients/agents.toml"),
    ),
    (
        "claude",
        include_str!("../../../../resources/clients/claude.toml"),
    ),
    (
        "codex",
        include_str!("../../../../resources/clients/codex.toml"),
    ),
    (
        "cursor",
        include_str!("../../../../resources/clients/cursor.toml"),
    ),
    (
        "opencode",
        include_str!("../../../../resources/clients/opencode.toml"),
    ),
    (
        "trae",
        include_str!("../../../../resources/clients/trae.toml"),
    ),
    (
        "trae-cn",
        include_str!("../../../../resources/clients/trae-cn.toml"),
    ),
    (
        "qoder",
        include_str!("../../../../resources/clients/qoder.toml"),
    ),
    (
        "workbuddy",
        include_str!("../../../../resources/clients/workbuddy.toml"),
    ),
    (
        "cline",
        include_str!("../../../../resources/clients/cline.toml"),
    ),
    (
        "copilot",
        include_str!("../../../../resources/clients/copilot.toml"),
    ),
    (
        "grok",
        include_str!("../../../../resources/clients/grok.toml"),
    ),
    (
        "antigravity",
        include_str!("../../../../resources/clients/antigravity.toml"),
    ),
];

#[derive(Debug, Clone, Deserialize)]
pub struct ClientToml {
    pub id: String,
    pub display_name: String,
    #[serde(default)]
    pub priority: i64,
    pub global_skills: String,
    pub project_skills: String,
    #[serde(default)]
    pub global_commands: Option<String>,
    #[serde(default)]
    pub project_commands: Option<String>,
    #[serde(default)]
    pub link_modes: Vec<String>,
    #[serde(default)]
    pub verified_at: Option<String>,
    #[serde(default)]
    pub verified_by: Option<String>,
    #[serde(default)]
    pub native_scan: Option<NativeScanToml>,
    /// When the client notices new files in its skill dirs: `"session"` = it only rescans when a
    /// new session opens (so a fresh projection needs a restart); unset = treated as hot-reloading
    /// and never produces `restart_required`. Marked per client only with real evidence.
    #[serde(default)]
    pub rescan: Option<String>,
    /// How to tell whether this client is installed on the machine we are running on. Declarative
    /// on purpose: the rules ship with the matrix, so correcting one misjudgement is a TOML change
    /// and not a release of the probe code. Absent = "we cannot tell", which the UI renders as it
    /// did before presence existed rather than as "not installed".
    ///
    /// Path rules differ from the scan fields: `global_skills`/`project_skills` are relative to the
    /// home or repo root, while `dirs` entries are taken **literally** except for a leading `~`,
    /// which is expanded. Absolute paths are allowed: some clients keep state under `/Library`.
    #[serde(default)]
    pub detect: Option<DetectToml>,
}

/// The three signal families, tried cheapest first (see `detect::run`).
#[derive(Debug, Clone, Default, Deserialize)]
pub struct DetectToml {
    /// Directories the client owns; `is_dir()` counts, so a symlinked dir is a hit.
    #[serde(default)]
    pub dirs: Vec<String>,
    /// macOS application bundles, looked up in `/Applications` and `~/Applications`.
    #[serde(default)]
    pub apps: Vec<String>,
    /// Binaries to run with a version flag. Every entry has a deadline.
    #[serde(default)]
    pub commands: Vec<CommandProbeToml>,
}

fn default_command_args() -> Vec<String> {
    vec!["--version".to_owned()]
}

#[derive(Debug, Clone, Deserialize)]
pub struct CommandProbeToml {
    pub bin: String,
    /// Defaults to `["--version"]` so omitting `args` in a matrix file never launches a bare
    /// interactive TUI/IDE binary until the probe deadline kills it.
    #[serde(default = "default_command_args")]
    pub args: Vec<String>,
    /// Defaults to [`crate::detect::DEFAULT_COMMAND_TIMEOUT_MS`].
    #[serde(default)]
    pub timeout_ms: Option<u64>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct NativeScanToml {
    #[serde(default)]
    pub supported: bool,
    #[serde(default)]
    pub evidence: String,
}

/// The built-in matrix, parsed once and cached for the lifetime of the process.
pub fn builtin() -> Result<Vec<ClientToml>> {
    static CACHE: OnceLock<Vec<ClientToml>> = OnceLock::new();
    if let Some(cached) = CACHE.get() {
        return Ok(cached.clone());
    }
    let mut matrix = Vec::with_capacity(BUILTIN_FILES.len());
    let mut seen: HashSet<String> = HashSet::new();
    for (name, text) in BUILTIN_FILES {
        let parsed = toml::from_str::<ClientToml>(text).map_err(|e| {
            heron_core::HeronError::from(
                heron_core::ErrorCode::Internal
                    .msg()
                    .with("detail", format!("clients/{name}.toml: {e}")),
            )
        })?;
        // Two files claiming one id would make the catalogue depend on iteration order, so the
        // mistake has to fail loudly here rather than silently decide which row wins.
        if !seen.insert(parsed.id.clone()) {
            return Err(heron_core::ErrorCode::Internal
                .msg()
                .with(
                    "detail",
                    format!("clients/{name}.toml: duplicate id {}", parsed.id),
                )
                .into());
        }
        matrix.push(parsed);
    }
    let _ = CACHE.set(matrix.clone());
    Ok(matrix)
}

pub fn specs(home: &Path) -> Result<Vec<heron_scan::classify::ClientSpec>> {
    Ok(builtin()?
        .into_iter()
        .map(|c| heron_scan::classify::ClientSpec {
            id: c.id,
            display_name: c.display_name,
            global_paths: vec![home.join(&c.global_skills)],
            project_paths: vec![PathBuf::from(&c.project_skills)],
            command_paths: c
                .global_commands
                .map(|p| vec![home.join(p)])
                .unwrap_or_default(),
            project_command_paths: c
                .project_commands
                .map(|p| vec![PathBuf::from(p)])
                .unwrap_or_default(),
            priority: c.priority,
        })
        .collect())
}

/// The catalogue columns of one matrix entry, as they go into `client`.
fn catalog_row(c: &ClientToml, home: &Path) -> ClientRow {
    ClientRow {
        id: c.id.clone(),
        display_name: c.display_name.clone(),
        global_paths: vec![home.join(&c.global_skills).to_string_lossy().to_string()],
        project_paths: vec![c.project_skills.clone()],
        link_modes: c.link_modes.clone(),
        global_commands: c
            .global_commands
            .as_ref()
            .map(|p| vec![home.join(p).to_string_lossy().to_string()])
            .unwrap_or_default(),
        project_commands: c
            .project_commands
            .as_ref()
            .map(|p| vec![p.clone()])
            .unwrap_or_default(),
        priority: c.priority,
        native_scan: c.native_scan.as_ref().map(|n| {
            serde_json::json!({ "supported": n.supported, "evidence": n.evidence }).to_string()
        }),
        verified_at: c.verified_at.clone(),
        verified_by: c.verified_by.clone(),
        // Presence is not a catalogue field; it has its own write path ([`apply_presence`]).
        detected: None,
        detected_at: None,
        detect_evidence: vec![],
    }
}

/// The global landing point of one matrix entry. No target is marked default: projecting into a
/// client directory is always an explicit choice.
fn global_target(c: &ClientToml, home: &Path) -> TargetRow {
    TargetRow {
        id: format!("{}:global", c.id),
        kind: c.id.clone(),
        scope: "global".into(),
        project_path: None,
        path: home.join(&c.global_skills).to_string_lossy().to_string(),
        enabled: true,
        is_default: false,
        discover: "auto".into(),
    }
}

/// Bring the stored catalogue in line with the compiled matrix.
///
/// `force = true` is the bootstrap path: every row is written as the matrix says. Without it, only
/// what is **missing** is created — a client row that is absent (a new release added a TOML) and a
/// global target whose client exists but whose row was never made. Nothing that exists is rewritten,
/// so an edit the user made to `display_name` or to a target survives a restart, and a re-sync can
/// never reset a presence verdict.
///
/// The client row is always written before the target row: `target.kind` has a foreign key onto
/// `client.id`, so the order is not cosmetic.
pub fn sync_catalog(db: &mut Db, home: &Path, force: bool) -> Result<()> {
    let matrix = builtin()?;
    let known_clients: HashSet<String> =
        client::list(db.conn())?.into_iter().map(|c| c.id).collect();
    let known_targets: HashSet<String> =
        target::list(db.conn())?.into_iter().map(|t| t.id).collect();
    db.write(|tx| {
        for c in &matrix {
            if force || !known_clients.contains(&c.id) {
                client::upsert(tx, &catalog_row(c, home))?;
            }
            let target_id = format!("{}:global", c.id);
            if force || !known_targets.contains(&target_id) {
                target::upsert(tx, &global_target(c, home))?;
            }
        }
        Ok(())
    })
}

/// Seed the client matrix on first run. Idempotent.
///
/// Heron's own `~/.heron/skills` is the store, not a projection target, so it never appears here.
/// Later runs only fill in what a newer binary added ([`sync_catalog`]), which is also what the scan
/// and the startup path call. It also drops the landing points an older release wrote by mistake
/// ([`prune_home_project_targets`]), so opening the index is enough to repair that history.
pub fn bootstrap(db: &mut Db, home: &Path) -> Result<()> {
    // Ahead of the first-run guard: every entry point opens the index through here, so a database
    // written by a release that made home a project is healed the moment it is opened, without the
    // user having to press 扫描 to see the duplicate cards go away.
    prune_home_project_targets(db, home)?;
    if db.kv_get("bootstrap_done")?.as_deref() == Some("1") {
        return Ok(());
    }
    sync_catalog(db, home, true)?;
    db.kv_set("bootstrap_done", "1")?;
    Ok(())
}

/// Store the verdicts of one detection run.
///
/// Narrow on purpose: only the three presence columns, and no targets are created here. The only
/// creator of client and target rows is [`sync_catalog`], which keeps the foreign-key order simple
/// and means a detection can never invent a landing point the user did not agree to.
pub fn apply_presence(db: &mut Db, detection: &crate::detect::Detection) -> Result<()> {
    db.write(|tx| {
        for verdict in &detection.verdicts {
            client::set_presence(
                tx,
                &verdict.id,
                verdict.detected,
                &detection.at,
                &verdict.evidence,
            )?;
        }
        Ok(())
    })
}

/// Forget the landing points that stopped being a directory on this machine.
///
/// A target is a place a client reads; once the directory is gone the card is noise in the overview
/// and its projection is a link nothing can be verified through. This is the tidy-up for the one
/// history a phantom row has: adopt booking the *resolved* directory behind a foreign link — inside
/// somebody else's repository — and that repository then being deleted (see
/// `crate::adopt::ensure_target_for_dir`, which no longer does it).
///
/// Conservative on purpose, because removing a target cascades to its projections and their cached
/// verdicts: only rows adopt wrote are considered (`discover = 'manual'`; the catalogue and the
/// project scan both write `'auto'`), and only when nothing is left on disk. An entry that still
/// exists — even as a broken link the user can repair — is never touched.
pub fn prune_phantom_targets(db: &mut Db) -> Result<usize> {
    let projections = projection::list_all(db.conn())?;
    let mut gone: Vec<String> = Vec::new();
    for t in target::list(db.conn())? {
        if t.discover != "manual" || entry_exists(Path::new(&t.path)) {
            continue;
        }
        if projections
            .iter()
            .any(|p| p.target_id == t.id && entry_exists(Path::new(&p.path)))
        {
            continue;
        }
        gone.push(t.id);
    }
    if gone.is_empty() {
        return Ok(0);
    }
    db.write(|tx| {
        for id in &gone {
            target::remove(tx, id)?;
        }
        Ok(())
    })?;
    Ok(gone.len())
}

/// Anything at all sitting at this path: a directory, a file, or a link — even a dangling one.
fn entry_exists(path: &Path) -> bool {
    std::fs::symlink_metadata(path).is_ok()
}

/// Forget the landing points that registered the home directory as a project.
///
/// [`crate::projects::discover`] used to report home itself as a project root, since the
/// `.<client>/skills` it holds match the discovery pattern. For every client whose `project_skills`
/// is the same relative path as its `global_skills`, that wrote a second, disabled landing point on
/// the very path the catalogue already registers — the overview then showed two identical cards for
/// one client. The walk no longer reports home, so this only clears what the old one left behind.
///
/// Conservative like [`prune_phantom_targets`]: a row that still owns projections is kept, because
/// deleting a target cascades to the links recorded under it.
pub fn prune_home_project_targets(db: &mut Db, home: &Path) -> Result<usize> {
    let projections = projection::list_all(db.conn())?;
    let mut stale: Vec<String> = Vec::new();
    for t in target::list(db.conn())? {
        if t.scope != "project" {
            continue;
        }
        let Some(root) = t.project_path.as_deref() else {
            continue;
        };
        if !heron_platform::paths::same_path_ci(Path::new(root), home) {
            continue;
        }
        if projections.iter().any(|p| p.target_id == t.id) {
            continue;
        }
        stale.push(t.id);
    }
    if stale.is_empty() {
        return Ok(0);
    }
    db.write(|tx| {
        for id in &stale {
            target::remove(tx, id)?;
        }
        Ok(())
    })?;
    Ok(stale.len())
}

/// Is this landing point live: switched on by the user *and* not reported absent by the last scan?
///
/// The single definition for the whole workspace, deliberately mirroring the three-state rule: an
/// unknown client (`detected = None`) is treated exactly as it was before presence existed, so
/// upgrading does not grey out every card until the first scan has run.
pub fn target_live(t: &TargetRow, clients: &[ClientRow]) -> bool {
    let verdict = clients
        .iter()
        .find(|c| c.id == t.kind)
        .and_then(|c| c.detected);
    t.enabled && verdict != Some(0)
}

/// Switch a landing point on or off — the consent flag behind [`target_live`]. Nothing on disk
/// moves: links already projected stay projected, and `verify` keeps reporting them, so the count a
/// target shows survives its switch. Turning it back on needs no re-projection.
pub fn set_target_enabled(db: &mut Db, target_id: &str, enabled: bool) -> Result<()> {
    db.write(|tx| target::set_enabled(tx, target_id, enabled))
}

/// List all registered targets.
pub fn list_targets(db: &Db) -> Result<Vec<TargetRow>> {
    target::list(db.conn())
}

/// List all clients in the catalogue with their presence verdicts.
pub fn list_clients(db: &Db) -> Result<Vec<ClientRow>> {
    client::list(db.conn())
}

/// Client specs as stored in the index (so user edits survive).
pub fn loaded_specs(db: &Db) -> Result<Vec<heron_scan::classify::ClientSpec>> {
    Ok(client::list(db.conn())?
        .into_iter()
        .map(|row| heron_scan::classify::ClientSpec {
            id: row.id,
            display_name: row.display_name,
            global_paths: row.global_paths.iter().map(PathBuf::from).collect(),
            project_paths: row.project_paths.iter().map(PathBuf::from).collect(),
            command_paths: row.global_commands.iter().map(PathBuf::from).collect(),
            project_command_paths: row.project_commands.iter().map(PathBuf::from).collect(),
            priority: row.priority,
        })
        .collect())
}

#[cfg(test)]
mod tests;
