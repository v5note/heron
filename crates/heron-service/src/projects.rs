//! Project discovery (decision D3: automatic **and** manual).
//!
//! A project root is registered when the scanner sees a `<proj>/.<client>/skills` directory.
//! Registration is a **record**, not a permission: nothing is projected into a project until the
//! user enables it, because project skills often come from somebody else's repository.

use std::path::{Path, PathBuf};

use heron_store::models::TargetRow;
use heron_store::repos::target;

use crate::{Ctx, Result};

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct DiscoveredProject {
    pub root: PathBuf,
    /// Client ids that showed up inside it.
    pub clients: Vec<String>,
    pub already_registered: bool,
}

/// Find project roots below the given scan roots.
pub fn discover(home: &Path, scan_roots: &[PathBuf], max_depth: usize) -> Vec<DiscoveredProject> {
    let mut found: Vec<DiscoveredProject> = Vec::new();
    for root in scan_roots {
        if !root.is_dir() {
            continue;
        }
        walk(root, home, max_depth, 0, &mut found);
    }
    found.sort_by(|a, b| a.root.cmp(&b.root));
    found.dedup_by(|a, b| a.root == b.root);
    found
}

fn walk(dir: &Path, home: &Path, max_depth: usize, depth: usize, out: &mut Vec<DiscoveredProject>) {
    if depth > max_depth {
        return;
    }
    let mut clients = Vec::new();
    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().to_string();
            if !name.starts_with('.') || name == "." || name == ".." {
                continue;
            }
            if entry.path().join("skills").is_dir() {
                clients.push(name.trim_start_matches('.').to_owned());
            }
        }
    }
    if !clients.is_empty() {
        out.push(DiscoveredProject {
            root: dir.to_path_buf(),
            clients: clients.clone(),
            already_registered: false,
        });
        // A project root is a boundary: do not descend into it looking for nested projects.
        return;
    }
    // Never walk the home directory itself beyond the first level of client dirs.
    if depth == 0 && dir == home {
        return;
    }
    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            let name = entry.file_name().to_string_lossy().to_string();
            if !path.is_dir() || name.starts_with('.') || name == "node_modules" || name == "target"
            {
                continue;
            }
            walk(&path, home, max_depth, depth + 1, out);
        }
    }
}

/// Register a project root (idempotent). Returns true when it was new.
///
/// Registration only *records* the project; projections into it stay off until enabled (D3).
pub fn register(ctx: &mut Ctx, root: &Path, clients: &[String]) -> Result<bool> {
    let already = target::list(ctx.db.conn())?.iter().any(|t| {
        t.scope == "project" && t.project_path.as_deref() == Some(root.to_str().unwrap_or_default())
    });
    if already {
        return Ok(false);
    }
    let specs = crate::clients::builtin()?;
    let rows: Vec<TargetRow> = clients
        .iter()
        .filter_map(|client_id| specs.iter().find(|c| &c.id == client_id))
        .map(|spec| TargetRow {
            id: format!(
                "{}:project:{}",
                spec.id,
                heron_platform::paths::project_key(root)
            ),
            kind: spec.id.clone(),
            scope: "project".to_owned(),
            project_path: Some(root.to_string_lossy().to_string()),
            path: root
                .join(&spec.project_skills)
                .to_string_lossy()
                .to_string(),
            // Off by default: a project's skills are not trusted input.
            enabled: false,
            is_default: false,
            discover: "auto".to_owned(),
        })
        .collect();
    ctx.db.write(|tx| {
        for row in &rows {
            target::upsert(tx, row)?;
        }
        Ok(())
    })?;
    Ok(!rows.is_empty())
}

/// Project roots already registered.
pub fn registered(ctx: &Ctx) -> Result<Vec<PathBuf>> {
    Ok(target::list(ctx.db.conn())?
        .into_iter()
        .filter(|t| t.scope == "project")
        .filter_map(|t| t.project_path.map(PathBuf::from))
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn discovers_project_roots_by_client_directory() {
        let tmp = tempfile::tempdir().unwrap();
        let home = tmp.path().join("home");
        let a = home.join("work/proj-a");
        let b = home.join("work/proj-b");
        std::fs::create_dir_all(a.join(".claude/skills")).unwrap();
        std::fs::create_dir_all(b.join(".agents/skills")).unwrap();
        std::fs::create_dir_all(home.join("empty")).unwrap();

        let found = discover(&home, &[home.join("work")], 3);
        let roots: Vec<_> = found.iter().map(|p| p.root.clone()).collect();
        assert!(roots.contains(&a));
        assert!(roots.contains(&b));
        let proj_a = found.iter().find(|p| p.root == a).unwrap();
        assert_eq!(proj_a.clients, vec!["claude".to_owned()]);
    }

    #[test]
    fn nested_projects_are_not_double_counted() {
        let tmp = tempfile::tempdir().unwrap();
        let home = tmp.path().join("home");
        let outer = home.join("work/outer");
        std::fs::create_dir_all(outer.join(".claude/skills")).unwrap();
        std::fs::create_dir_all(outer.join("packages/inner/.claude/skills")).unwrap();

        let found = discover(&home, &[home.join("work")], 4);
        assert_eq!(
            found.len(),
            1,
            "the outer project is the boundary: {found:?}"
        );
    }

    #[test]
    fn registers_one_target_per_client_and_stays_off_by_default() {
        use heron_platform::dirs::Dirs;
        use heron_store::db::Db;

        let tmp = tempfile::tempdir().unwrap();
        let data = tmp.path().join("data");
        std::fs::create_dir_all(&data).unwrap();
        let dirs = Dirs::resolve_with_override("heron", Some(&data)).unwrap();
        dirs.ensure().unwrap();
        let db = Db::open(&dirs.db_path()).unwrap();
        let mut ctx = Ctx::new(dirs, db, "cli");
        crate::clients::bootstrap(&mut ctx.db, Path::new("/home/u")).unwrap();

        let project = tmp.path().join("proj");
        std::fs::create_dir_all(&project).unwrap();
        assert!(register(&mut ctx, &project, &["claude".to_owned()]).unwrap());
        assert!(
            !register(&mut ctx, &project, &["claude".to_owned()]).unwrap(),
            "idempotent"
        );

        let targets = target::list(ctx.db.conn()).unwrap();
        let project_targets: Vec<_> = targets.iter().filter(|t| t.scope == "project").collect();
        assert_eq!(project_targets.len(), 1);
        assert!(
            !project_targets[0].enabled,
            "project projections need explicit consent"
        );
        assert_eq!(registered(&ctx).unwrap(), vec![project]);
    }
}
