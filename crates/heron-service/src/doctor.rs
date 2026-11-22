//! Doctor: the check registry (design §8.2 D).
//!
//! Every check is read-only and reports an `Issue` with a suggested action. Nested suspects
//! (decision D8) are *only* reported: Heron never reorganizes a user's tree on its own.

use std::path::PathBuf;

use serde::Serialize;

use crate::{Ctx, Result};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Severity {
    Info,
    Warn,
    Error,
}

#[derive(Debug, Clone, Serialize)]
pub struct Issue {
    /// Stable check id, e.g. `nested-suspect`, `broken-link`, `duplicate-content`.
    pub id: String,
    pub severity: Severity,
    pub subject: String,
    /// Suggested actions the user can take; never executed automatically.
    pub actions: Vec<String>,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct DoctorReport {
    pub issues: Vec<Issue>,
}

impl DoctorReport {
    pub fn count(&self, severity: Severity) -> usize {
        self.issues
            .iter()
            .filter(|i| i.severity == severity)
            .count()
    }
}

pub fn run(ctx: &Ctx) -> Result<DoctorReport> {
    let mut issues = Vec::new();

    // 1) Items on disk that Heron does not manage.
    for row in heron_store::repos::detected::list_by_state(ctx.db.conn(), "candidate")? {
        if row.kind == "nested-suspect" {
            issues.push(Issue {
                id: "nested-suspect".to_owned(),
                severity: Severity::Warn,
                subject: row.path.clone(),
                actions: vec![
                    "ignore".to_owned(),
                    "open-directory".to_owned(),
                    "export-diagnostics".to_owned(),
                ],
            });
        }
        if row.frontmatter_ok == Some(false) {
            issues.push(Issue {
                id: "frontmatter-invalid".to_owned(),
                severity: Severity::Warn,
                subject: row.path.clone(),
                actions: vec!["open-directory".to_owned()],
            });
        }
    }

    // 2) Broken projections.
    for row in heron_store::repos::projection::list_all(ctx.db.conn())? {
        let path = PathBuf::from(&row.path);
        if !path.exists() {
            issues.push(Issue {
                id: "broken-link".to_owned(),
                severity: Severity::Error,
                subject: row.path.clone(),
                actions: vec!["reproject".to_owned(), "unproject".to_owned()],
            });
        }
    }

    // 3) Duplicate content across managed skills (cheap check: same store dir name count).
    let skills = heron_store::repos::skill::list(ctx.db.conn(), false)?;
    let mut seen: std::collections::HashMap<String, String> = Default::default();
    for skill in &skills {
        let dir = ctx.content_path(skill);
        if !dir.is_dir() {
            issues.push(Issue {
                id: "store-missing".to_owned(),
                severity: Severity::Error,
                subject: skill.slug.clone(),
                actions: vec!["restore-from-backup".to_owned(), "remove".to_owned()],
            });
            continue;
        }
        if let Ok(fp) = heron_scan::fingerprint_dir(&dir, &Default::default()) {
            if let Some(other) = seen.insert(fp.hash.to_string(), skill.slug.clone()) {
                issues.push(Issue {
                    id: "duplicate-content".to_owned(),
                    severity: Severity::Info,
                    subject: format!("{} == {}", other, skill.slug),
                    actions: vec!["merge".to_owned()],
                });
            }
        }
    }

    Ok(DoctorReport { issues })
}
