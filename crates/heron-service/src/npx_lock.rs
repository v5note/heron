//! Upstream provenance left behind by third-party installers.
//!
//! `npx skills` records where each entry came from in a lockfile sitting next to (or above) the
//! skills directory. Heron takes no ownership of those files — it never writes them and refuses to
//! project into a directory that has one without saying so — but on adopt it must not throw the
//! information away: it is the only record of "this content came from `alchaincyf/huashu-design`".
//! Without it every installed skill degraded into an untraceable local copy that `outdated` and
//! `upgrade` could never service.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::Deserialize;

/// Where an entry was installed from, as far as an installer recorded it.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct Upstream {
    /// Fetchable URL; `None` when the installer only knew a local or bundled source.
    pub url: Option<String>,
    /// Directory inside the repository holding this skill (`None` means the repository root).
    pub subpath: Option<String>,
    /// Which lockfile answered, so the UI can say where the answer came from.
    pub via: String,
    /// The installer's own content fingerprint. It is *not* a git revision, so it must never be
    /// used as a merge baseline; it is kept as a note only.
    pub installer_hash: Option<String>,
}

#[derive(Debug, Deserialize)]
struct LockFile {
    #[serde(default)]
    skills: BTreeMap<String, LockEntry>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct LockEntry {
    #[serde(default)]
    source: String,
    #[serde(default)]
    source_type: String,
    #[serde(default)]
    source_url: Option<String>,
    #[serde(default)]
    skill_path: Option<String>,
    #[serde(default)]
    computed_hash: Option<String>,
    #[serde(default)]
    skill_folder_hash: Option<String>,
}

/// The upstream an installer recorded for `<…>/<name>`, looked up by the entry's directory name.
pub fn for_entry(entry_dir: &Path) -> Option<Upstream> {
    let name = entry_dir.file_name()?.to_string_lossy().to_string();
    for lock in candidate_locks(entry_dir) {
        let Ok(text) = std::fs::read_to_string(&lock) else {
            continue;
        };
        let Ok(parsed) = serde_json::from_str::<LockFile>(&text) else {
            continue;
        };
        let Some(entry) = parsed.skills.get(&name) else {
            continue;
        };
        let hash = entry
            .computed_hash
            .as_ref()
            .or(entry.skill_folder_hash.as_ref())
            .filter(|h| !h.is_empty())
            .cloned();
        return Some(Upstream {
            url: url_of(entry),
            subpath: subpath_of(entry),
            via: lock.file_name()?.to_string_lossy().to_string(),
            installer_hash: hash,
        });
    }
    None
}

/// `<skills dir>/skills-lock.json`, then the `.skill-lock.json` one level up — the two shapes the
/// ecosystem has shipped.
fn candidate_locks(entry_dir: &Path) -> Vec<PathBuf> {
    let Some(dir) = entry_dir.parent() else {
        return vec![];
    };
    let mut out = vec![dir.join("skills-lock.json")];
    out.push(dir.join(".skill-lock.json"));
    if let Some(home) = dir.parent() {
        out.push(home.join(".skill-lock.json"));
    }
    out
}

fn url_of(entry: &LockEntry) -> Option<String> {
    if let Some(url) = entry
        .source_url
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        return Some(url.to_owned());
    }
    let source = entry.source.trim();
    if source.is_empty() {
        return None;
    }
    // Anything that is not a `owner/repo` pair (a path, an archive name) has no fetchable upstream.
    let is_repo = matches!(entry.source_type.as_str(), "github" | "gitlab" | "")
        && source.contains('/')
        && !source.starts_with('.')
        && !source.contains(char::is_whitespace);
    if !is_repo {
        return None;
    }
    let host = if entry.source_type == "gitlab" {
        "gitlab.com"
    } else {
        "github.com"
    };
    Some(format!("https://{host}/{source}.git"))
}

fn subpath_of(entry: &LockEntry) -> Option<String> {
    let raw = entry.skill_path.as_deref().map(str::trim)?;
    if raw.is_empty() {
        return None;
    }
    let parent = Path::new(raw).parent()?;
    let text = parent.to_string_lossy().replace('\\', "/");
    (!text.is_empty() && text != ".").then_some(text)
}
