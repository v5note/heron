//! Per-skill version control (design ADR-5).
//!
//! Every managed skill is its own git repository inside the store:
//!
//! * `main` — the working branch (local edits are committed here);
//! * `upstream` — the last upstream state Heron materialized (Heron-only writes);
//! * `v<version>` tags — one per landed upstream release (rollback targets);
//! * `heron.skill.json` — metadata committed with the skill (baseline for three-way merges).
//!
//! Merge strategy (implementation doc §5.5): `git merge-tree --write-tree --merge-base=<base>`
//! when git >= 2.38, otherwise a per-file three-way merge.

pub mod git;
pub mod merge;
pub mod repo;

pub use git::Git;
pub use heron_core::{HeronError, Msg, Result};
pub use merge::{ConflictFile, MergeOutcome};
pub use repo::{CommitInfo, MetaSource, SkillMeta, SkillRepo};
