//! Per-thread branded heap core: the [`RawHeap`] state and its concern
//! submodules.
//!
//! The type is split by responsibility so each leaf owns one path:
//! - [`alloc`] — the small-class fast route and large/huge fallback.
//! - [`free`] — small-page free-list return and large/huge release.
//! - [`realloc`] — grow/shrink dispatch and in-place reuse decision.
//! - [`state`] — the shared struct shape, its `Send`/`Default` impls, and
//!   the public `new`/`alloc`/`stats` entry points.

mod alloc;
mod free;
mod realloc;
mod state;

pub(crate) use state::RawHeap;
