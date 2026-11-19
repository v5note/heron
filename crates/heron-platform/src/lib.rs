//! Platform layer.
//!
//! This is the **only** crate in the workspace that may contain `#[cfg(windows)]` or
//! `#[cfg(unix)]` (implementation doc §1.2). Everything risky about file systems lives
//! behind these modules so that the rest of Heron can be written and tested once.

#![deny(unsafe_op_in_unsafe_fn)]

pub mod atomic;
pub mod dirs;
pub mod link;
pub mod lock;
pub mod paths;
pub mod probe;
pub mod time;

pub use heron_core::{HeronError, Msg, Result};
