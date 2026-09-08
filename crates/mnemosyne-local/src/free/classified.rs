//! The classified free path: block, page, and segment bookkeeping.

use super::cold::thread_free_cold;
use super::internal::do_local_free_internal_policy;
use crate::free_helpers::commit_in_place_free;
use crate::{LocalAllocatorSelector, poison_freed_bytes};
use core::ptr::NonNull;
use mnemosyne_arena::{HasSegmentPool, deallocate_large_or_huge};
use mnemosyne_core::constants::PAGE_SIZE;
use mnemosyne_core::policy::AllocPolicy;
use mnemosyne_core::types::{Block, Page, Segment, locate_page, locate_segment};
#[inline(always)]
pub(super) unsafe fn thread_free_classified<
    P: AllocPolicy,
    B: HasSegmentPool + LocalAllocatorSelector<B>,
    const LAYOUT_PROVES_SMALL: bool,
>(
    ptr: *mut u8,
) {
    if ptr.is_null() {
        return;
    }

    let ptr_val = ptr as usize;
    // SAFETY: `ptr` was previously returned by this allocator, satisfying
    // `locate_segment`'s contract; it recovers the live segment header and the
    // bounded page index.
    let (segment, page_index) = unsafe { locate_segment(ptr) };

    // SAFETY: `segment` and `page_index` were validated by `locate_segment`.
    // `locate_page` avoids retaining a reference-derived metadata tag across
    // the alloc/free boundary of the shared metadata-and-payload mapping.
    let page_ptr = unsafe { locate_page(segment, page_index) };
    if mnemosyne_prof::is_active() {
        unsafe { record_free_profile(ptr, page_ptr, page_index) };
    }

    // `page_index == 0` short-circuits before the metadata read: page 0 is
    // never allocated from, so a zero index means `ptr` is segment-aligned and
    // the address `locate_segment` masked to is payload rather than a header.
    // Reading `block_size` from it would interpret user bytes as page metadata.
    // See `usable_size` for the full argument.
    if !LAYOUT_PROVES_SMALL && (page_index == 0 || unsafe { (*page_ptr).block_size } == 0) {
        // SAFETY: huge-allocation metadata layout. `segment` is recovered
        // from the metadata slot one pointer slot directly preceding the
        // user payload (`(ptr as *mut *mut Segment) - 1`); every huge
        // allocation writes this slot at `allocate_large_or_huge` time.
        // The `pages[0].alloc_count` / `huge_mapping_suffix_from` reads,
        // the `poison_freed_bytes` write, and the `deallocate_large_or_huge`
        // call all stay inside the originating huge mapping.
        let segment = unsafe { *((ptr as *mut *mut Segment).sub(1)) };
        if P::ENABLE_POISONING {
            let size = unsafe { (*segment).pages[0].alloc_count };
            let size = if size > 0 {
                size
            } else {
                unsafe { (*segment).huge_mapping_suffix_from(ptr) }
            };
            // SAFETY: covered by the huge-allocation metadata argument above: `segment` is
            // the originating mapping and `size` its extent from `ptr`, so the poison write
            // and the release stay inside that mapping.
            unsafe { poison_freed_bytes::<P>(ptr, size) };
        }
        let _released = unsafe { deallocate_large_or_huge::<B>(ptr, segment) };
        #[cfg(feature = "dealloc-probe")]
        crate::dealloc_counters::record(crate::dealloc_counters::DeallocPath::HugeClassifier);
        return;
    }

    // SAFETY: `page_ptr` is the live page metadata `locate_page` recovered for
    // `ptr`'s segment and index; `block_size` is written once at page
    // initialization and only read here.
    debug_assert_eq!(
        (ptr_val & (PAGE_SIZE - 1)) % unsafe { (*page_ptr).block_size },
        0,
        "small free ptr must be aligned to the page's block stride"
    );

    // Record per-size-class dealloc telemetry before poison overwrites the block.
    // `size_to_class_nonzero` is a single table lookup (O(1)); Relaxed atomics
    // mean no memory-fence overhead on the hot path.
    {
        use mnemosyne_core::size_class::size_to_class_nonzero;
        let block_size = unsafe { (*page_ptr).block_size };
        if let Some(class) = size_to_class_nonzero(block_size) {
            crate::bin_stats::record_dealloc(class);
        }
    }

    // SAFETY: `ptr` is the block being freed — `block_size` bytes this free owns
    // exclusively until the block re-enters a free list — so the poison write
    // stays inside the block.
    if P::ENABLE_POISONING {
        unsafe { poison_freed_bytes::<P>(ptr, (*page_ptr).block_size) };
    }

    let block = ptr as *mut Block;
    // SAFETY: `segment` is the live mapping `locate_segment` recovered for `ptr`;
    // `owner` reads its ownership token, which is immutable while the segment
    // is mapped.
    let owner = unsafe { Segment::owner(segment) };

    #[cfg(all(windows, target_arch = "x86_64", not(miri)))]
    let (is_owner, owner_allocator) = {
        let tid = mnemosyne_core::types::current_thread_id();
        if owner.matches_thread_id(tid) {
            // SAFETY: `segment` is live (above) and `owner` matched this thread's id, so
            // the owner-allocator pointer names this thread's own allocator.
            (true, unsafe { Segment::owner_allocator(segment) })
        } else {
            (false, core::ptr::null_mut())
        }
    };
    #[cfg(any(not(all(windows, target_arch = "x86_64")), miri))]
    let (is_owner, owner_allocator) = {
        let standard_allocator = B::get_allocator_ptr_raw_for_encryption::<false>();
        let encrypted_allocator = B::get_allocator_ptr_raw_for_encryption::<true>();
        if owner.matches(standard_allocator) {
            (true, standard_allocator)
        } else if owner.matches(encrypted_allocator) {
            (true, encrypted_allocator)
        } else {
            (false, core::ptr::null_mut())
        }
    };

    if is_owner && !owner_allocator.is_null() {
        // SAFETY: `page_ptr` is live (above) and this thread owns the segment, so no
        // other thread writes `alloc_count` while it is read.
        let page_alloc_count = unsafe { (*page_ptr).alloc_count };
        if page_alloc_count == 0 {
            std::process::abort();
        }
        // SAFETY: `block` is a user pointer previously returned by the
        // allocator; non-nullness is the allocator invariant. Equality
        // with `page.free` is the double-free guard.
        if Some(unsafe { NonNull::new_unchecked(block) }) == unsafe { (*page_ptr).free } {
            std::process::abort();
        }
        // `owner_allocator` is the owner token, which by the slot's offset-0
        // invariant is also this thread's slot address — so the re-entrancy
        // gate is reachable from it without borrowing the allocator.
        // SAFETY: the surrounding `is_owner && !owner_allocator.is_null()` was
        // just confirmed against `segment.owner`, so this is the current
        // thread's own live slot and no concurrent accessors exist.
        let is_allocating =
            unsafe { crate::tls_slot::LocalAllocatorSlot::<B>::is_allocating(owner_allocator) };
        let page_free = unsafe { (*page_ptr).free };
        // SAFETY: `segment`/`page_index` locate this page's parent header and its
        // key slot, satisfying `cookie_for`'s contract.
        let encrypted = unsafe { Segment::free_list_encrypted(segment) };
        let cookie = unsafe { Segment::cookie_for_dynamic(segment, encrypted, page_index) };

        if unsafe { (*page_ptr).list_state } != 2 {
            // Page is active
            // Ask the segment, not the allocator. `is_current` is the owner's
            // own mirror of `current_segment`, maintained by
            // `set_current_segment`, and this path runs even while the gate is
            // raised — reading it through the allocator would need a borrow the
            // gate exists to forbid. Same form as the `is_current` reads in
            // `occupancy` and the cold path below.
            // SAFETY: `is_owner` was confirmed above, so `segment` is this
            // thread's live, owned header.
            if page_alloc_count > 1 || unsafe { Segment::is_current(segment) } {
                // Free in-place (either remains active, or is current segment).
                // SAFETY: `block` is non-null by the alloc_count / page.free
                // corruption guards above, and `page_alloc_count == page.free`'s
                // owning count; the shared commit stays inside this owned page.
                unsafe {
                    commit_in_place_free(
                        block,
                        page_ptr,
                        page_free,
                        cookie,
                        encrypted,
                        page_alloc_count,
                    )
                };
                #[cfg(feature = "dealloc-probe")]
                crate::dealloc_counters::record(crate::dealloc_counters::DeallocPath::InPlaceSmall);
                return;
            } else if !is_allocating {
                // Page is not the current segment and this free empties it. The
                // free-list head set, the segment-aware decrement, and the
                // active→empty page-list transition are the shared commit in
                // `do_local_free_internal`; the caller adds only the re-entrancy
                // guard and the sweep-cadence bump around it.
                // SAFETY: `owner_allocator` is non-null and belongs to this thread (the
                // `is_owner` branch), so flipping its TLS re-entrancy flag is a same-thread
                // write on a slot that outlives this call.
                unsafe {
                    crate::tls_slot::LocalAllocatorSlot::<B>::set_allocating(owner_allocator, true)
                };
                // Borrow only now: the gate was false and is now raised, so no
                // other `&mut` to this cache is live.
                // SAFETY: this thread's own slot (offset-0 invariant), gate
                // checked and raised.
                let alloc = unsafe {
                    crate::tls_slot::LocalAllocatorSlot::<B>::allocator_mut(owner_allocator)
                };
                // SAFETY: `block`/`page`/`segment`/`page_index` are the validated
                // free-path inputs (guards above) with `alloc` the owning
                // allocator — exactly `do_local_free_internal`'s contract.
                let _became_empty = unsafe {
                    do_local_free_internal_policy::<P, B>(
                        alloc, block, page_ptr, segment, page_index,
                    )
                };
                // SAFETY: `alloc` is the exclusively-borrowed owning allocator
                // with `is_allocating` raised, the precondition of the cold sweep.
                unsafe { alloc.record_defrag_operation::<P>(true) };
                unsafe {
                    crate::tls_slot::LocalAllocatorSlot::<B>::set_allocating(owner_allocator, false)
                };
                #[cfg(feature = "dealloc-probe")]
                crate::dealloc_counters::record(
                    crate::dealloc_counters::DeallocPath::ActiveFreeLastBlock,
                );
                return;
            }
        } else if !is_allocating {
            // Page is full, transitions to active (count > 1 is guaranteed since
            // max_blocks >= 8, so it never empties directly). This is the
            // full→active branch of the shared `do_local_free_internal` commit.
            // SAFETY: this thread's own slot (offset-0 invariant), and the gate
            // read false, so no other `&mut` to this cache is live.
            let alloc =
                unsafe { crate::tls_slot::LocalAllocatorSlot::<B>::allocator_mut(owner_allocator) };
            // SAFETY: as above — validated free-path inputs and the owning
            // `alloc`, satisfying `do_local_free_internal`'s contract.
            let _became_empty = unsafe {
                do_local_free_internal_policy::<P, B>(alloc, block, page_ptr, segment, page_index)
            };
            #[cfg(feature = "dealloc-probe")]
            crate::dealloc_counters::record(crate::dealloc_counters::DeallocPath::FullToActive);
            return;
        }
    }

    // SAFETY: `ptr`/`page_ptr`/`block` are the function's validated
    // contract inputs from the embodiment of `thread_free`'s `// # Safety`
    // rustdoc; the `#[cold]` helper handles the cross-thread / re-entrant
    // push path.
    unsafe { thread_free_cold::<B>(ptr, page_ptr, block) };
}

#[cold]
#[inline(never)]
unsafe fn record_free_profile(ptr: *mut u8, page: *const Page, page_index: usize) {
    let block_size = unsafe { (*page).block_size };
    let size = if page_index == 0 || block_size == 0 {
        // Large/huge allocation: recover the size from the shared metadata-slot
        // accessor.
        // SAFETY: `page_index == 0 || block_size == 0` identifies a large/huge
        // allocation whose metadata slot precedes `ptr`, satisfying
        // `huge_allocation_size`'s precondition.
        unsafe { crate::usable_size::huge_allocation_size(ptr) }
    } else {
        block_size
    };
    mnemosyne_prof::on_free(ptr, size);
}
