use crate::free_helpers::{commit_in_place_free, is_sole_active_page};
use crate::local_alloc::page::{
    move_page_between_lists_branded, push_page_front, unlink_page_from_list, with_page_list_token,
};
use crate::per_cpu;
use crate::{LocalAllocatorSelector, ThreadAllocator, poison_freed_bytes};
use core::ptr::NonNull;
use mnemosyne_arena::{HasSegmentPool, deallocate_large_or_huge};
use mnemosyne_core::constants::PAGE_SIZE;
use mnemosyne_core::policy::AllocPolicy;
use mnemosyne_core::types::{Block, Page, Segment, locate_page, locate_segment};

/// Frees a memory block.
///
/// # Safety
///
/// The ptr must be valid and must have been returned by a previous allocation.
/// A null pointer is ignored, matching `free(NULL)`.
///
/// # Examples
///
/// ```
/// use mnemosyne_local::{thread_alloc, thread_free};
/// use mnemosyne_core::StandardPolicy;
/// use mnemosyne_backend::MemoryBackendWrapper as Backend;
///
/// // SAFETY: `p` comes from `thread_alloc` and is freed exactly once. Freeing
/// // it twice, or freeing a pointer this allocator did not return, is
/// // undefined behaviour that the allocator aborts on when it detects it.
/// unsafe {
///     let p = thread_alloc::<StandardPolicy, Backend>(32, 8);
///     assert!(!p.is_null());
///     thread_free::<StandardPolicy, Backend>(p);
///
///     // Freeing null is a no-op, so callers need no guard of their own.
///     thread_free::<StandardPolicy, Backend>(core::ptr::null_mut());
/// }
/// ```
#[inline(always)]
pub unsafe fn thread_free<P: AllocPolicy, B: HasSegmentPool + LocalAllocatorSelector<B>>(
    ptr: *mut u8,
) {
    // SAFETY: forwarded under `thread_free`'s own contract — `ptr` came from this
    // allocator and is freed once; `false` keeps the unclassified path.
    unsafe { thread_free_classified::<P, B, false>(ptr) }
}

/// Frees a memory block when the caller has a valid Rust `Layout`.
///
/// The layout-proven small path monomorphizes out the large/huge classifier
/// branch while retaining the raw `thread_free` fallback for large, huge, or
/// unusual-alignment allocations.
///
/// # Safety
///
/// Same contract as [`thread_free`], and `size`/`align` must come from the
/// original allocation layout.
/// # Examples
///
/// ```
/// use core::alloc::Layout;
/// use mnemosyne_local::{thread_alloc_layout, thread_free_layout};
/// use mnemosyne_core::StandardPolicy;
/// use mnemosyne_backend::MemoryBackendWrapper as Backend;
///
/// let layout = Layout::from_size_align(96, 16).expect("96/16 is a valid layout");
///
/// // SAFETY: the `size`/`align` passed to the free are the ones the
/// // allocation was made with; a mismatched layout would misroute the free.
/// unsafe {
///     let p = thread_alloc_layout::<StandardPolicy, Backend>(layout.size(), layout.align());
///     assert!(!p.is_null());
///     thread_free_layout::<StandardPolicy, Backend>(p, layout.size(), layout.align());
/// }
/// ```
#[inline(always)]
pub unsafe fn thread_free_layout<P: AllocPolicy, B: HasSegmentPool + LocalAllocatorSelector<B>>(
    ptr: *mut u8,
    size: usize,
    align: usize,
) {
    // Derive the layout-proven small fast path from the same routing decision
    // `alloc` used, so the two never disagree on whether a block is small
    // (a disagreement would treat a huge allocation as small — UB). This now
    // also covers `align > MIN_BLOCK_SIZE` small allocations served by the
    // alignment-aware small path.
    // SAFETY: `thread_free_layout`'s contract holds (`ptr` from this allocator, freed
    // once, `size`/`align` as allocated), so the small-path classification below is
    // the one `alloc` made and the chosen arm frees the block on the path that
    // produced it.
    if size != 0 && crate::alloc::small_path_class(size, align).is_some() {
        unsafe { thread_free_classified::<P, B, true>(ptr) };
    } else {
        unsafe { thread_free_classified::<P, B, false>(ptr) };
    }
}

#[inline(always)]
unsafe fn thread_free_classified<
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

    // SAFETY: `ptr` is the block being freed — `block_size` bytes this free owns
    // exclusively until the block re-enters a free list — so the poison write
    // stays inside the block.
    if P::ENABLE_POISONING {
        unsafe { poison_freed_bytes::<P>(ptr, (*page_ptr).block_size) };
    }

    // Record per-size-class dealloc telemetry before the block re-enters the
    // free list. `size_to_class_nonzero` is a single table lookup (O(1));
    // Relaxed atomics mean no memory-fence overhead on the hot path.
    {
        use mnemosyne_core::size_class::size_to_class_nonzero;
        let block_size = unsafe { (*page_ptr).block_size };
        if let Some(class) = size_to_class_nonzero(block_size) {
            crate::bin_stats::record_dealloc(class);
        }
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
                    do_local_free_internal::<B>(alloc, block, page_ptr, segment, page_index)
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
            let _became_empty =
                unsafe { do_local_free_internal::<B>(alloc, block, page_ptr, segment, page_index) };
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

#[cold]
#[inline(never)]
unsafe fn thread_free_cold<B: HasSegmentPool + LocalAllocatorSelector<B>>(
    ptr: *mut u8,
    page: *mut Page,
    block: *mut Block,
) {
    // SAFETY: `page` is the live page metadata the caller located for a block of
    // this allocator, and `parent_segment_of` masks it to its live segment header;
    // both reads are of initialization-time fields.
    let encrypted = unsafe { Segment::free_list_encrypted(Page::parent_segment_of(page)) };
    if B::ENABLE_CPU_CACHE
        && per_cpu::try_free_cpu(ptr, unsafe { (*page).size_class } as usize, encrypted)
    {
        #[cfg(feature = "dealloc-probe")]
        crate::dealloc_counters::record(crate::dealloc_counters::DeallocPath::ColdOrRecursing);
        return;
    }

    // SAFETY: `block` came from this allocator under the same
    // backend; non-nullness is the allocator invariant. The page-
    // local atomic free list takes ownership of the pointer.
    unsafe {
        (*page)
            .thread_free
            .push_dynamic(NonNull::new_unchecked(block), encrypted);
    }
    #[cfg(feature = "dealloc-probe")]
    crate::dealloc_counters::record(crate::dealloc_counters::DeallocPath::ColdOrRecursing);
}

/// Internal implementation of local deallocation.
///
/// # Safety
///
/// The block pointer must point to a valid block allocated in the target page and segment.
#[inline(always)]
pub unsafe fn do_local_free_internal<B: HasSegmentPool>(
    alloc: &mut ThreadAllocator<B>,
    block: *mut Block,
    page: *mut Page,
    segment: *mut Segment,
    page_index: usize,
) -> bool {
    // SAFETY: `page` is valid per this function's contract, and the caller holds
    // the owner's exclusive page-list access, so `alloc_count` has no concurrent
    // writer.
    if unsafe { (*page).alloc_count } == 0 {
        std::process::abort();
    }
    // SAFETY: `block` is a user pointer the `# Safety` contract guarantees was
    // returned by a prior allocation in `page`/`segment`; non-nullness is the
    // allocator invariant, so `new_unchecked` is sound. Equality with
    // `page.free` is the double-free guard (the head was just freed).
    if Some(unsafe { NonNull::new_unchecked(block) }) == unsafe { (*page).free } {
        std::process::abort();
    }
    let was_full = unsafe { (*page).list_state } == 2;
    // SAFETY: `segment` is the live segment header owning `page` per the
    // `# Safety` contract and `page_index` is this page's index, satisfying
    // `cookie_for`'s contract.
    let encrypted = unsafe { Segment::free_list_encrypted(segment) };
    let cookie = unsafe { Segment::cookie_for_dynamic(segment, encrypted, page_index) };
    // SAFETY: `block` points to a valid block in `page` per the `# Safety`
    // contract; writing its embedded next pointer reinitializes the free-list
    // link and stays inside the block this caller now owns.
    unsafe {
        (*block).set_next_dynamic((*page).free, encrypted, cookie);
    }
    // SAFETY: `block` is non-null (allocator invariant, re-confirmed by the
    // double-free guard above); publishing it as the new free-list head.
    unsafe { (*page).free = Some(NonNull::new_unchecked(block)) };

    // SAFETY: `segment`/`page`/`page_index` are the matching segment, page, and
    // its index per the `# Safety` contract; the decrement updates this page's
    // and segment's occupancy bookkeeping under the caller's exclusive access.
    let becomes_empty = unsafe {
        let count = (*page).alloc_count - 1;
        (*page).alloc_count = count;
        if count == 0 && !Segment::is_current(segment) {
            (*segment).page_occupied_mask &= !(1 << page_index);
        }
        count == 0
    };

    let class = unsafe { (*page).size_class } as usize;
    let page_ptr = unsafe { NonNull::new_unchecked(page) };

    with_page_list_token::<B, _>(|mut token| {
        // SAFETY: `page_ptr` is non-null (built above from the contract-valid `page`)
        // and belongs to the page lists the token brands.
        let branded_page = unsafe { token.page(page_ptr) };
        // SAFETY: the transitions below take the token that proves exclusive access to
        // this allocator's page lists, and `page`'s `list_state` names the list it
        // currently sits in, so unlink/move operate on a linked node.
        if was_full {
            if becomes_empty && !alloc.is_current_segment(segment) {
                // Case 1: Went from full directly to empty
                unsafe {
                    unlink_page_from_list(
                        &mut token,
                        alloc.full_pages.get_unchecked_mut(class),
                        branded_page,
                    );
                    push_page_front(&mut token, &mut alloc.empty_pages, branded_page, 3);
                }
            } else {
                // Case 2: Went from full to active
                unsafe {
                    move_page_between_lists_branded(
                        &mut token,
                        alloc.full_pages.get_unchecked_mut(class),
                        alloc.active_pages.get_unchecked_mut(class),
                        branded_page,
                        1,
                    );
                }
            }
        } else if becomes_empty && !alloc.is_current_segment(segment) {
            // Case 3: Went from active to empty (only if not the only active page)
            // SAFETY: `active_pages[class]` is this thread's own active-list head
            // and `page` is its live, owner-exclusive page, so the predicate's
            // head read is valid.
            let is_only_active =
                unsafe { is_sole_active_page(*alloc.active_pages.get_unchecked(class), page) };
            if !is_only_active {
                unsafe {
                    unlink_page_from_list(
                        &mut token,
                        alloc.active_pages.get_unchecked_mut(class),
                        branded_page,
                    );
                    push_page_front(&mut token, &mut alloc.empty_pages, branded_page, 3);
                }
            }
        }
    });

    becomes_empty
}
