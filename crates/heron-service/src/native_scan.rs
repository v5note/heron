//! Client native-scan injection (design §10.11, decision D6).
//!
//! The rule that makes this safe: clients are **capability-table driven**. A client whose TOML
//! says `supported = false` is never written to, no matter what the user clicks. When a client
//! does advertise support, injection still requires an explicit call, shows a diff first, backs
//! the file up and can be reverted.

use std::path::{Path, PathBuf};

use heron_core::{ErrorCode, Result};
use serde::Serialize;

use crate::clients;
use crate::Ctx;

#[derive(Debug, Clone, Serialize)]
pub struct NativeScanStatus {
    pub client: String,
    pub display_name: String,
    pub supported: bool,
    pub evidence: String,
    pub config_path: Option<String>,
    /// `injected` | `absent` | `drifted`
    pub state: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct InjectPlan {
    pub client: String,
    pub config_path: String,
    pub diff: String,
    pub backup_path: String,
}

/// What Heron would add to a client's configuration: the store directory as an extra scan path.
fn payload_for(ctx: &Ctx) -> String {
    serde_json::json!({
        "heron": {
            "version": env!("CARGO_PKG_VERSION"),
            "extra_skill_dirs": [ctx.dirs.store().to_string_lossy()],
        }
    })
    .to_string()
}

pub fn status() -> Result<Vec<NativeScanStatus>> {
    Ok(clients::builtin()?
        .into_iter()
        .map(|c| {
            let native = c.native_scan.clone();
            NativeScanStatus {
                client: c.id.clone(),
                display_name: c.display_name.clone(),
                supported: native.as_ref().map(|n| n.supported).unwrap_or(false),
                evidence: native.map(|n| n.evidence).unwrap_or_default(),
                config_path: None,
                state: "absent".to_owned(),
            }
        })
        .collect())
}

fn config_path_for(client: &clients::ClientToml, home: &Path) -> PathBuf {
    // Only reachable for clients whose table declares support. When a client is verified, its
    // TOML gains the real config file name and this function reads it from there.
    home.join(format!(".{}/heron.json", client.id))
}

/// Build the diff Heron would apply. Never writes.
pub fn plan(ctx: &Ctx, home: &Path, client_id: &str) -> Result<InjectPlan> {
    let matrix = clients::builtin()?;
    let client = matrix.iter().find(|c| c.id == client_id).ok_or_else(|| {
        ErrorCode::Internal
            .msg()
            .with("detail", "unknown_client")
            .with("client", client_id.to_owned())
    })?;

    let supported = client
        .native_scan
        .as_ref()
        .map(|n| n.supported)
        .unwrap_or(false);
    if !supported {
        return Err(ErrorCode::NativeScanUnsupported
            .msg()
            .with("client", client_id.to_owned())
            .with(
                "evidence",
                client
                    .native_scan
                    .as_ref()
                    .map(|n| n.evidence.clone())
                    .unwrap_or_default(),
            )
            .into());
    }

    let path = config_path_for(client, home);
    let before = std::fs::read_to_string(&path).unwrap_or_default();
    let after = payload_for(ctx);
    // A readable unified-ish diff: removed lines from the current file, added lines from ours.
    let removed: String = before.lines().map(|l| format!("-{l}\n")).collect();
    let added: String = after.lines().map(|l| format!("+{l}\n")).collect();
    Ok(InjectPlan {
        client: client_id.to_owned(),
        config_path: path.to_string_lossy().to_string(),
        diff: format!(
            "--- {}\n+++ {}\n{removed}{added}",
            path.display(),
            path.display()
        ),
        backup_path: ctx
            .dirs
            .backups()
            .join("native-scan")
            .join(client_id)
            .join("config.bak")
            .to_string_lossy()
            .to_string(),
    })
}

/// Apply a plan: back up, write atomically. Requires `config.native_scan.enabled`.
pub fn apply(ctx: &Ctx, plan: &InjectPlan, enabled: bool) -> Result<()> {
    if !enabled {
        return Err(ErrorCode::NativeScanDisabled
            .msg()
            .with("client", plan.client.clone())
            .into());
    }
    let target = PathBuf::from(&plan.config_path);
    if target.is_file() {
        let backup = PathBuf::from(&plan.backup_path);
        if let Some(parent) = backup.parent() {
            std::fs::create_dir_all(parent).ok();
        }
        std::fs::copy(&target, &backup).map_err(|e| {
            heron_core::HeronError::from(
                ErrorCode::Permission
                    .msg()
                    .with("path", backup.display().to_string())
                    .with("detail", e.to_string()),
            )
        })?;
    }
    heron_platform::atomic::write_atomic(&target, payload_for(ctx).as_bytes())?;
    Ok(())
}

/// Restore the previous configuration from the backup.
pub fn revert(_ctx: &Ctx, plan: &InjectPlan) -> Result<()> {
    let backup = PathBuf::from(&plan.backup_path);
    let target = PathBuf::from(&plan.config_path);
    if backup.is_file() {
        std::fs::copy(&backup, &target).map_err(|e| {
            heron_core::HeronError::from(
                ErrorCode::Permission
                    .msg()
                    .with("path", target.display().to_string())
                    .with("detail", e.to_string()),
            )
        })?;
    } else if target.is_file() {
        std::fs::remove_file(&target).map_err(|e| {
            heron_core::HeronError::from(
                ErrorCode::Permission
                    .msg()
                    .with("path", target.display().to_string())
                    .with("detail", e.to_string()),
            )
        })?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use heron_platform::dirs::Dirs;
    use heron_store::db::Db;

    fn ctx(root: &Path) -> Ctx {
        let data = root.join("data");
        std::fs::create_dir_all(&data).unwrap();
        let dirs = Dirs::resolve_with_override("heron", Some(&data)).unwrap();
        dirs.ensure().unwrap();
        let db = Db::open(&dirs.db_path()).unwrap();
        Ctx::new(dirs, db, "cli")
    }

    #[test]
    fn unsupported_clients_are_never_touched() {
        let tmp = tempfile::tempdir().unwrap();
        let ctx = ctx(tmp.path());
        let err = plan(&ctx, tmp.path(), "claude").unwrap_err();
        assert_eq!(err.code(), "E_NATIVE_SCAN_UNSUPPORTED");
        assert!(!tmp.path().join(".claude/heron.json").exists());
    }

    #[test]
    fn injection_requires_the_switch() {
        let tmp = tempfile::tempdir().unwrap();
        let ctx = ctx(tmp.path());
        let plan = InjectPlan {
            client: "demo".into(),
            config_path: tmp.path().join("cfg.json").to_string_lossy().to_string(),
            diff: String::new(),
            backup_path: tmp.path().join("bak.json").to_string_lossy().to_string(),
        };
        let err = apply(&ctx, &plan, false).unwrap_err();
        assert_eq!(err.code(), "E_NATIVE_SCAN_DISABLED");
        assert!(
            !tmp.path().join("cfg.json").exists(),
            "disabled means nothing is written"
        );
    }

    #[test]
    fn apply_then_revert_restores_the_original() {
        let tmp = tempfile::tempdir().unwrap();
        let ctx = ctx(tmp.path());
        let config = tmp.path().join("cfg.json");
        std::fs::write(&config, "{\"original\":true}").unwrap();
        let plan = InjectPlan {
            client: "demo".into(),
            config_path: config.to_string_lossy().to_string(),
            diff: String::new(),
            backup_path: tmp.path().join("bak.json").to_string_lossy().to_string(),
        };
        apply(&ctx, &plan, true).unwrap();
        let written = std::fs::read_to_string(&config).unwrap();
        assert!(
            written.contains("extra_skill_dirs"),
            "store path injected: {written}"
        );

        revert(&ctx, &plan).unwrap();
        assert_eq!(
            std::fs::read_to_string(&config).unwrap(),
            "{\"original\":true}"
        );
    }

    #[test]
    fn status_reports_every_client_as_unsupported_today() {
        let rows = status().unwrap();
        assert_eq!(rows.len(), 5);
        assert!(rows.iter().all(|r| !r.supported && !r.evidence.is_empty()));
    }
}
