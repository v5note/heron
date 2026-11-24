//! Remote git access, deliberately small (implementation doc §5.6).
//!
//! Heron keeps one **bare cache per remote URL** and never clones into the store directly:
//!
//! * `ensure()` fetches `--depth=200` of every head, the tags and the remote's default branch
//!   into `<cache>/<sha256(url)>/bare.git`;
//! * `resolve()` / `heads()` / `tags()` read refs without touching the network;
//! * `checkout_subdir()` materializes one skill directory with `git archive`, which needs no
//!   working tree and therefore never litters the cache with half-checked-out files.
//!
//! Every network operation runs through `Git::run_network`, which carries the proxy Heron
//! resolved (see `heron_platform::proxy`) and kills a stalled transfer instead of hanging the
//! caller until the user gives up.

pub mod cache;
pub mod remote;

pub use cache::{cache_dir_for, CachedRepo, TreeEntry, TreeEntryKind};
pub use heron_core::{HeronError, Msg, Result};
pub use remote::{RefKind, RemoteUrl, ResolvedRef};
