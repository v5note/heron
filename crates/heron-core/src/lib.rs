//! Heron domain model.
//!
//! Hard rules (see docs/IMPLEMENTATION-heron-v1.md §1.2):
//! - no IO, no async, no system clock;
//! - no user-facing prose: every message carries a stable `code` plus parameters
//!   and is localized by the UI (implementation doc §9).

#![forbid(unsafe_code)]

pub mod dto;
pub mod error;
pub mod hash;
pub mod ids;
pub mod platform_kind;
pub mod policy;
pub mod slug;
pub mod version;

pub use error::{ErrorCode, HeronError, Msg, Result, Severity};
pub use hash::Hash;
pub use ids::{ClientId, MarketId, SourceId, TargetId};
pub use platform_kind::PlatformKind;
pub use policy::{ConflictPolicy, PinPolicy, ProjectionMode, UpgradeLane};
pub use slug::{Slug, SlugPolicy};
pub use version::Version;
