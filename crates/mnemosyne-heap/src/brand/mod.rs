mod block;
mod branded_cell;
mod scopes;

// Brand vocabulary re-exported from melinoe so the heap's branded containers and
// their consumers share one authoritative token + marker definition.
pub use block::BrandedBlock;
pub use branded_cell::BrandedCell;
pub use melinoe::InvariantLifetime;
pub use melinoe::sync::{SyncRegionToken, ThreadLocalToken};
pub use scopes::{scope, sync_scope};
