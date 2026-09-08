//! Items the sibling crates call through, kept out of the crate's
//! documented entry-point list.

pub use crate::ThreadAllocator;
pub use crate::options::ensure_options_initialized;
// Neither of these is an entry point. `mark_options_initialized` is the
// seam the `mnemosyne` crate uses to freeze options it configured itself,
// and `reset_options_for_testing` exists only for tests. They live here so
// the crate root lists what a consumer actually calls.
pub use crate::options::{mark_options_initialized, reset_options_for_testing};
pub use crate::{
    do_local_free_internal, do_local_free_internal_policy, initialize_allocated_bytes,
    poison_freed_bytes, small_realloc_fits_existing_class, thread_free_layout,
};
pub use core::alloc::Layout;
pub use core::ptr::NonNull;
pub use mnemosyne_arena::HasSegmentPool;
pub use mnemosyne_arena::{allocate_large_or_huge, deallocate_large_or_huge};
pub use mnemosyne_core::constants::{
    MAX_SMALL_ALLOC_SIZE, MIN_BLOCK_SIZE, PAGE_SHIFT, PAGES_PER_SEGMENT, SEGMENT_SIZE,
};
pub use mnemosyne_core::size_class::size_to_class_nonzero;
pub use mnemosyne_core::types::{Block, Page, Segment};
pub use mnemosyne_core::validation::is_valid_layout_alloc_request;
