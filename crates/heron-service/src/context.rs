//! Shared state handed to every use case.

use std::path::PathBuf;

use heron_platform::dirs::Dirs;
use heron_store::db::Db;

pub struct Ctx {
    pub dirs: Dirs,
    pub db: Db,
    /// Identity recorded on audit events (`cli` / `gui`).
    pub actor: &'static str,
}

impl Ctx {
    pub fn new(dirs: Dirs, db: Db, actor: &'static str) -> Self {
        Self { dirs, db, actor }
    }

    pub fn store_root(&self) -> PathBuf {
        self.dirs.store()
    }

    pub fn skill_path(&self, slug: &str) -> PathBuf {
        self.store_root().join(slug)
    }

    pub fn backups_dir(&self) -> PathBuf {
        self.dirs
            .backups()
            .join(heron_platform::time::now_rfc3339().replace([':', '.'], "-"))
    }
}
