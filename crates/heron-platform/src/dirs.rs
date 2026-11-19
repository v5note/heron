//! User data, config, cache and log directories (design ADR-8).
//!
//! Windows deliberately uses `LOCALAPPDATA` for data so that OneDrive roaming cannot turn
//! junctions into cloud placeholders.

use std::path::{Path, PathBuf};

use heron_core::{ErrorCode, Msg};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Dirs {
    pub data: PathBuf,
    pub config: PathBuf,
    pub cache: PathBuf,
    pub log: PathBuf,
}

impl Dirs {
    /// Resolve the standard locations for `app` (e.g. `heron`).
    pub fn resolve(app: &str) -> Result<Self, Msg> {
        let pd = directories::ProjectDirs::from("io", "greyheron", app)
            .ok_or_else(|| ErrorCode::Internal.msg().with("detail", "no_home_dir"))?;
        let data = pd.data_local_dir().to_path_buf();
        Ok(Self {
            config: pd.config_dir().to_path_buf(),
            cache: pd.cache_dir().to_path_buf(),
            log: data.join("log"),
            data,
        })
    }

    /// Data directory can be relocated (e.g. to an external disk); the other three follow
    /// the platform defaults.
    pub fn resolve_with_override(app: &str, data_override: Option<&Path>) -> Result<Self, Msg> {
        let mut dirs = Self::resolve(app)?;
        if let Some(dir) = data_override {
            if dir.as_os_str().is_empty() {
                return Err(ErrorCode::Internal
                    .msg()
                    .with("detail", "empty_data_override"));
            }
            // An explicit data directory owns everything Heron writes, so portable installs,
            // tests and CI never need access outside it.
            dirs.data = dir.to_path_buf();
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

    pub fn store(&self) -> PathBuf {
        self.data.join("store")
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

/// Default landing directory owned by Heron (decision D10).
pub const DEFAULT_LANDING_DIRNAME: &str = ".heron/skills";

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
        assert_eq!(d.store(), d.data.join("store"));
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
    fn default_landing_is_heron_owned() {
        // Decision D10: Heron's own landing dir, not ~/.agents/skills.
        assert!(DEFAULT_LANDING_DIRNAME.starts_with(".heron"));
        assert!(!DEFAULT_LANDING_DIRNAME.contains(".agents"));
    }
}
