//! Use cases.
//!
//! The service layer owns ordering and side effects; the crates below it stay pure. Every
//! mutating use case follows the same shape:
//!
//! 1. compute a plan (never touching the disk);
//! 2. let the caller confirm it (`dry_run` in the CLI, a preview dialog in the GUI);
//! 3. execute, recording an `event` with undo steps so a crash or a change of mind is recoverable.
//!
//! Implementation doc §7.4.

pub mod adopt;
pub mod clients;
pub mod context;
pub mod diff;
pub mod doctor;
pub mod install;
pub mod lockfile;
pub mod native_scan;
pub mod project;
pub mod projects;
pub mod scan;
pub mod upgrade;
pub mod verify;

pub use context::Ctx;
pub use heron_core::{HeronError, Msg, Result};
