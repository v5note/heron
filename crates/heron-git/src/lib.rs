//! Remote git access, deliberately small (implementation doc §5.6).
//!
//! Heron keeps one **bare cache per remote URL** and never clones into the store directly:
//!
//! * `ensure()` fetches `--depth=1` into `<cache>/<sha256(url)>/bare.git`;
//! * `resolve_ref()` / `heads()` / `tags()` read refs without touching the network;
//! * `checkout_subdir()` materializes one skill directory with `git archive`, which needs no
//!   working tree and therefore never litters the cache with half-checked-out files.
//!
//! Every network operation is cancellable-by-timeout and inherits the user's proxy/credential
//! configuration through the normal git environment.

pub mod cache;
pub mod remote;

pub use cache::{cache_dir_for, CachedRepo, TreeEntry, TreeEntryKind};
pub use heron_core::{HeronError, Msg, Result};
pub use remote::{RefKind, RemoteUrl, ResolvedRef};
