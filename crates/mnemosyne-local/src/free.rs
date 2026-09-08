//! Deallocation routing: entry points, the classified fast path, the
//! cold page/segment transitions, and the allocator-in-hand helpers.

mod classified;
mod cold;
mod entry;
mod internal;

pub use entry::{thread_free, thread_free_layout};
pub use internal::{do_local_free_internal, do_local_free_internal_policy};
