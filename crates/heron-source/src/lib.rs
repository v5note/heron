//! Sources.
//!
//! A user can hand us four shapes of input (design §9.2) and Heron must always turn them into
//! an explicit `(repo, ref, subpath)` triple — or say plainly that it is ambiguous.

pub mod catalog;
pub mod layout;
pub mod market;
pub mod url;

pub use catalog::{Catalog, CatalogItem, CatalogSource};
pub use heron_core::{HeronError, Msg, Result};
pub use layout::{detect as detect_layout, SkillLayout};
pub use market::{adapter, Capability, InstallPlan, MarketAdapter, MarketInfo, MARKETS};
pub use url::{parse_source, ParsedSource, SourceKind};
