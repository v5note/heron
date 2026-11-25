//! Shared state handed to every use case.

use std::path::PathBuf;

use heron_platform::dirs::Dirs;
use heron_store::db::Db;
use heron_store::models::SkillRow;

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

    /// Where the authoritative content of `row` actually lives: its store copy, or the external
    /// directory a live-bound skill was adopted from without being moved.
    pub fn content_path(&self, row: &SkillRow) -> PathBuf {
        match row.storage_path.as_deref().filter(|p| !p.trim().is_empty()) {
            Some(p) => std::path::PathBuf::from(p),
            None => self.skill_path(&row.slug),
        }
    }

    /// `content_path` for callers that only know the slug. Unknown slugs keep the store default.
    pub fn content_path_of(&self, slug: &str) -> PathBuf {
        heron_store::repos::skill::get(self.db.conn(), slug)
            .ok()
            .flatten()
            .map(|row| self.content_path(&row))
            .unwrap_or_else(|| self.skill_path(slug))
    }

    /// The directory holding `SKILL.md`: for a whole-repository skill the versioned root plus the
    /// recorded `subpath`, otherwise the content path itself.
    pub fn skill_dir(&self, row: &SkillRow) -> PathBuf {
        match row.subpath.as_deref().filter(|p| !p.trim().is_empty()) {
            Some(sub) => self.content_path(row).join(sub),
            None => self.content_path(row),
        }
    }

    /// `skill_dir` for callers that only know the slug. Unknown slugs keep the content default.
    pub fn skill_dir_of(&self, slug: &str) -> PathBuf {
        heron_store::repos::skill::get(self.db.conn(), slug)
            .ok()
            .flatten()
            .map(|row| self.skill_dir(&row))
            .unwrap_or_else(|| self.content_path_of(slug))
    }

    /// What a client link should point at: the skill entry, never the surrounding checkout root.
    pub fn projectable_path_of(&self, slug: &str) -> PathBuf {
        self.skill_dir_of(slug)
    }

    pub fn backups_dir(&self) -> PathBuf {
        self.dirs
            .backups()
            .join(heron_platform::time::now_rfc3339().replace([':', '.'], "-"))
    }
}
