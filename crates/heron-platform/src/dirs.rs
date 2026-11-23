//! User home, data, config, cache and log directories (design ADR-8).
//!
//! The store of managed skills is not inside the data directory: it is `~/.heron/skills`, the one
//! place skill content lives, mapped out to the client directories by projection.
//!
//! Windows deliberately uses `LOCALAPPDATA` for data so that OneDrive roaming cannot turn
//! junctions into cloud placeholders.

use std::path::{Path, PathBuf};

use heron_core::{ErrorCode, Msg};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Dirs {
    /// The user's home directory; the store lives under it ([`Dirs::store`]).
    pub home: PathBuf,
    pub data: PathBuf,
    pub config: PathBuf,
    pub cache: PathBuf,
    pub log: PathBuf,
}

impl Dirs {
    /// Resolve the standard locations for `app` (e.g. `heron`).
    pub fn resolve(app: &str) -> Result<Self, Msg> {
        let pd = directories::ProjectDirs::from("xin", "v5ai", app)
            .ok_or_else(|| ErrorCode::Internal.msg().with("detail", "no_home_dir"))?;
        // `directories` 6 gives no home through `ProjectDirs`, so ask `BaseDirs`; its constructor
        // already resolves $HOME and the passwd entry, and absence is the same hard error as above.
        let home = directories::BaseDirs::new()
            .map(|base| base.home_dir().to_path_buf())
            .ok_or_else(|| ErrorCode::Internal.msg().with("detail", "no_home_dir"))?;
        let data = pd.data_local_dir().to_path_buf();
        Ok(Self {
            home,
            config: pd.config_dir().to_path_buf(),
            cache: pd.cache_dir().to_path_buf(),
            log: data.join("log"),
            data,
        })
    }

    /// Data directory can be relocated (e.g. to an external disk); config, cache, log and the store
    /// follow it, the rest stays on the platform defaults.
    pub fn resolve_with_override(app: &str, data_override: Option<&Path>) -> Result<Self, Msg> {
        let mut dirs = Self::resolve(app)?;
        if let Some(dir) = data_override {
            if dir.as_os_str().is_empty() {
                return Err(ErrorCode::Internal
                    .msg()
                    .with("detail", "empty_data_override"));
            }
            // An explicit data directory owns everything Heron writes, so portable installs,
            // tests and CI never need access outside it. That includes the store: it lives in the
            // home directory by default, so `home` follows the override too and never points at
            // the real `~/.heron`.
            dirs.data = dir.to_path_buf();
            dirs.home = dir.to_path_buf();
            dirs.config = dirs.data.join("config");
            dirs.cache = dirs.data.join("cache");
            dirs.log = dirs.data.join("log");
        }
        Ok(dirs)
    }

    /// Create every directory. Safe to call on every start.
    pub fn ensure(&self) -> Result<(), Msg> {
        for dir in [&self.data, &self.config, &self.cache, &self.log] {
            std::fs::create_dir_all(dir).map_err(io_msg(dir))?;
        }
        Ok(())
    }

    /// The store: the one and only place every managed skill lives.
    pub fn store(&self) -> PathBuf {
        self.home.join(HERON_DIRNAME).join(STORE_DIRNAME)
    }
    pub fn backups(&self) -> PathBuf {
        self.data.join("backups")
    }
    pub fn trash(&self) -> PathBuf {
        self.data.join("trash")
    }
    pub fn sources_cache(&self) -> PathBuf {
        self.data.join("sources-cache")
    }
    pub fn db_path(&self) -> PathBuf {
        self.data.join("heron.db")
    }
    pub fn config_path(&self) -> PathBuf {
        self.data.join("config.toml")
    }
}

pub(crate) fn io_msg(path: &Path) -> impl Fn(std::io::Error) -> Msg + '_ {
    move |e| {
        ErrorCode::Permission
            .msg()
            .with("path", path.display().to_string())
            .with("detail", e.to_string())
    }
}

/// The directory Heron owns inside the user's home, holding the store and nothing else.
///
/// Kept as two separate names rather than one `".heron/skills"` string: `Path::join` appends the
/// whole argument, so a literal slash would survive on Windows as `C:\Users\you\.heron/skills`.
/// Joining segment by segment gives the platform-correct `%USERPROFILE%\.heron\skills`.
pub const HERON_DIRNAME: &str = ".heron";
/// The store directory within [`HERON_DIRNAME`]: the single place every managed skill's content is
/// kept. It is deliberately Heron's own directory and never a client's, so the store is not one of
/// the paths the scanner walks (no self-scan) and stays decoupled from `npx skills`, which uses
/// `~/.agents`.
pub const STORE_DIRNAME: &str = "skills";

#[cfg(test)]
mod tests {
    use super::*;

    fn uses_own_dir(name: &str) -> bool {
        // directories v6 composes "<qualifier>.<organization>.<name>[-<suffix>]",
        // so the app name is a substring rather than a suffix.
        Dirs::resolve(name)
            .unwrap()
            .data
            .to_string_lossy()
            .contains(name)
    }

    #[test]
    fn resolves_all_four_directories() {
        assert!(uses_own_dir("heron-test-dirs"));
        let d = Dirs::resolve("heron-test-dirs").expect("home dir available");
        assert!(d.log.starts_with(&d.data));
        // Only compared, never created: this test must not turn the real ~/.heron into a target.
        assert_eq!(d.store(), d.home.join(HERON_DIRNAME).join(STORE_DIRNAME));
        assert_eq!(d.db_path(), d.data.join("heron.db"));
        assert_eq!(d.backups(), d.data.join("backups"));
        assert_eq!(d.trash(), d.data.join("trash"));
        assert_eq!(d.sources_cache(), d.data.join("sources-cache"));
        assert_eq!(d.config_path(), d.data.join("config.toml"));
    }

    #[test]
    fn override_owns_every_directory() {
        let tmp = Path::new("/tmp/heron-data-override");
        let d = Dirs::resolve_with_override("heron-test-dirs", Some(tmp)).unwrap();
        assert_eq!(d.data, tmp);
        assert_eq!(d.log, tmp.join("log"));
        assert_eq!(
            d.config,
            tmp.join("config"),
            "an explicit data dir owns config too"
        );
        assert_eq!(d.cache, tmp.join("cache"));
        assert_eq!(d.db_path(), tmp.join("heron.db"));
        assert_eq!(d.home, tmp, "an explicit data dir owns the store too");
        assert_eq!(d.store(), tmp.join(".heron").join("skills"));
    }

    #[test]
    fn empty_override_is_rejected() {
        assert!(Dirs::resolve_with_override("heron", Some(Path::new(""))).is_err());
    }

    #[test]
    fn ensure_creates_directories() {
        let tmp = tempfile::tempdir().unwrap();
        let d = Dirs::resolve_with_override("heron-test-ensure", Some(tmp.path())).unwrap();
        // point config/cache at the temp dir too so the test never touches ~/Library
        let d = Dirs {
            config: tmp.path().join("config"),
            cache: tmp.path().join("cache"),
            ..d
        };
        d.ensure().unwrap();
        assert!(d.data.is_dir());
        assert!(d.log.is_dir());
        assert!(d.config.is_dir());
        assert!(d.cache.is_dir());
    }

    #[test]
    fn store_dir_is_heron_owned() {
        // The store is Heron's own directory, never a client's: it must not be one of the paths the
        // scanner walks, and it stays decoupled from `npx skills`, which owns ~/.agents.
        assert_eq!(HERON_DIRNAME, ".heron");
        assert_eq!(STORE_DIRNAME, "skills");
        assert!(!HERON_DIRNAME.contains("agents"));
        // Each segment must be a single component: a separator inside them would survive as a
        // foreign `/` on Windows instead of being rendered with `\`.
        assert!(
            !HERON_DIRNAME.contains('/') && !STORE_DIRNAME.contains('\\'),
            "store path must be joined segment by segment"
        );
    }
}
