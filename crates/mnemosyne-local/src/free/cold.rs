//! The cold free path taken when a page or segment changes state.

use crate::LocalAllocatorSelector;
use crate::per_cpu;
use core::ptr::NonNull;
use mnemosyne_arena::HasSegmentPool;
use mnemosyne_core::types::{Block, Page, Segment};

#[cold]
#[inline(never)]
pub(super) unsafe fn thread_free_cold<B: HasSegmentPool + LocalAllocatorSelector<B>>(
    ptr: *mut u8,
    page: *mut Page,
    block: *mut Block,
    segment: *mut Segment,
    page_index: usize,
) {
    // SAFETY: `segment` is the live header the caller located for `block`; the
    // encryption flag is an initialization-time field.
    let encrypted = unsafe { Segment::free_list_encrypted(segment) };
    if B::ENABLE_CPU_CACHE
        && per_cpu::try_free_cpu(ptr, unsafe { (*page).size_class } as usize, encrypted)
    {
        #[cfg(feature = "dealloc-probe")]
        crate::dealloc_counters::record(crate::dealloc_counters::DeallocPath::ColdOrRecursing);
        return;
    }

    // SAFETY: `block` came from this allocator under the same
    // backend; non-nullness is the allocator invariant. The page-
    // local atomic free list takes ownership of the pointer, and
    // `segment`/`page_index` are the pair the caller located for it.
    unsafe {
        (*page).thread_free.push_located(
            NonNull::new_unchecked(block),
            segment,
            page_index,
            encrypted,
        );
    }
    #[cfg(feature = "dealloc-probe")]
    crate::dealloc_counters::record(crate::dealloc_counters::DeallocPath::ColdOrRecursing);
}
