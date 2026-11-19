//! Projection: the only way a skill leaves the store.
//!
//! * [`plan`] computes what would happen without touching the disk (GUI preview = CLI `--dry-run`);
//! * [`apply`] executes a plan through the platform link layer, refusing anything unsafe;
//! * [`verify`] answers the question that matters: *would a client actually load this?*
//!
//! Design §7.1 / §10.1; implementation doc §7.1.

pub mod apply;
pub mod budget;
pub mod plan;
pub mod verify;

pub use budget::{estimate_tokens, BudgetEstimate};
pub use heron_core::{HeronError, Msg, Result};
pub use plan::{plan_projection, PlanAction, ProjectPlan, ProjectionRequest};

/// Re-exported so callers do not depend on the scan crate directly.
pub use verify::{VerifyInput, Visibility};
