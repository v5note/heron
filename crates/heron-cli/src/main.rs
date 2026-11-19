//! `heron` command line.
//!
//! M0 scope: `scan`, `list`, `detected`. Every command supports `--json` and returns the
//! shared DTOs, so the future GUI consumes exactly the same data (implementation doc §7.6).

use std::process::ExitCode;

mod app;
mod render;

fn main() -> ExitCode {
    match app::run() {
        Ok(code) => code,
        Err(err) => {
            // Errors are structured; the CLI renders them once, here.
            let msg = match &err {
                heron_core::HeronError::Msg(m) => m.clone(),
                heron_core::HeronError::Io(e) => heron_core::ErrorCode::Internal
                    .msg()
                    .with("detail", e.to_string()),
            };
            eprintln!(
                "error[{}]: {}",
                msg.code,
                serde_json::to_string(&msg.params).unwrap_or_default()
            );
            ExitCode::from(1)
        }
    }
}
