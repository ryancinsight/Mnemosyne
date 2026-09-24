//! Shared page-state helpers for the free path.

//! Extracted from `free.rs` so the hot free entry points and the shared
//! page-list primitives stay individually readable; these two are consumed
//! cross-module by `realloc.rs` and `local_alloc::segment::reclaim`.

use core::ffi::c_void;
use core::ptr::NonNull;

use mnemosyne_core::types::{Block, Page, Segment, SegmentOwner};

/// Returns true when `page` is the single page linked in the active list rooted
/// at `active_head` (it is the head and has no successor).
///
/// This is the "do not strand the last active page in the empty list" guard
/// shared by the local-free active→empty transition and the defragmentation
/// sweep; keeping it linked as an active page lets the next allocation of this
/// class reuse it without a cold refill.
///
/// # Safety
///
/// `active_head` must be the head of an intrusive active-page list owned by the
/// calling thread, and `page` must be a live page pointer from that thread's
/// allocator, so the head dereference is a valid, unaliased read.
#[inline(always)]
pub(crate) unsafe fn is_sole_active_page(
    active_head: Option<NonNull<Page>>,
    page: *const Page,
) -> bool {
    active_head.is_some_and(|head| {
        // SAFETY: `head` is a live, owner-exclusive page pointer per the caller's
        // contract; reading `next_page` is a valid shared read.
        core::ptr::eq(head.as_ptr(), page) && unsafe { (*head.as_ptr()).next_page.is_none() }
    })
}

/// Commits an in-place block free onto a page that keeps its list membership:
/// links `block` at the front of the page-local free list and decrements the
/// live count without touching any page-list or segment-occupancy state.
///
/// This is the hot "page stays active / is the current slicing segment" arm,
/// shared by `thread_free` and the small-realloc old-block free. Because the
/// page neither empties (its count stays `>= 1`) nor changes list, the plain
/// `alloc_count` decrement is correct: `decrement_alloc_count_for_segment` would
/// touch the occupancy mask only on the `count == 0` transition, which does not
/// occur here.
///
/// # Safety
///
/// `block` must be a live, non-null block previously allocated in `page` (its
/// double-free/underflow guards must already have passed), `page_free`/`cookie`
/// must be `page`'s current free-list head and encryption cookie, and
/// `page_alloc_count` must be `page.alloc_count` (`>= 1`).
#[inline(always)]
pub(crate) unsafe fn commit_in_place_free(
    block: *mut Block,
    page: *mut Page,
    _page_free: Option<NonNull<Block>>,
    cookie: usize,
    encrypted: bool,
    page_alloc_count: usize,
    randomized: bool,
) {
    // SAFETY: `block` is a live, non-null block owned by `page` per the caller's
    // contract; the free-list head mutation stays inside that page.
    unsafe {
        let (current_head, use_secondary) =
            Page::choose_free_head(page, page_alloc_count, randomized);
        (*block).set_next_dynamic(current_head, encrypted, cookie);
        if use_secondary {
            (*page).secondary_free = Some(NonNull::new_unchecked(block));
        } else {
            (*page).free = Some(NonNull::new_unchecked(block));
        }
        (*page).alloc_count = (page_alloc_count - 1) as u32;
    }
}

/// Resolves the owning allocator slot for a segment, applying platform-specific
/// thread-identity detection. This is the **SSOT** for the ownership-resolution
/// pattern shared by the free and realloc paths.
///
/// Returns `(is_owner, owner_slot_ptr)` where:
/// - `is_owner` — whether the calling context owns this segment's allocator
/// - `owner_slot_ptr` — the concrete slot pointer to use for in-place
///   free/realloc (`null` when `is_owner` is `false`)
///
/// On Windows x86-64 an additional `Segment::owner_allocator` check catches
/// same-thread cross-policy access (e.g. a `StandardPolicy` caller freeing
/// into a `HardenedPolicy`-owned segment). All other targets rely only on the
/// caller's TLS slot matching the segment's owner token.
///
/// # Safety
///
/// `segment` must point to a live, initialized segment header.
#[inline]
pub(crate) unsafe fn resolve_owner_slot(
    segment: *mut Segment,
    owner: SegmentOwner,
    slot_ptr: *mut c_void,
) -> (bool, *mut c_void) {
    #[cfg(all(windows, target_arch = "x86_64", not(miri)))]
    {
        // SAFETY: `segment` is a live header; `owner_allocator` reads only the
        // atomic ownership field via raw-pointer projection.
        let tid = mnemosyne_core::types::current_thread_id();
        let owner_allocator = unsafe { Segment::owner_allocator(segment) };
        let same_thread_owner = !owner_allocator.is_null() && owner.matches_thread_id(tid);
        let caller_owner = !slot_ptr.is_null() && owner.matches(slot_ptr);
        if same_thread_owner {
            return (true, owner_allocator);
        }
        if caller_owner {
            return (true, slot_ptr);
        }
        return (false, core::ptr::null_mut());
    }
    #[cfg(not(all(windows, target_arch = "x86_64", not(miri))))]
    {
        let caller_owner = !slot_ptr.is_null() && owner.matches(slot_ptr);
        if caller_owner {
            (true, slot_ptr)
        } else {
            (false, core::ptr::null_mut())
        }
    }
}

/// Non-generic SSOT for the large/huge free path.
///
/// Recovers the owning segment from the metadata slot preceding `ptr`,
/// optionally poisons the block, and releases the segment mapping.
///
/// Both `classified.rs` (in `mnemosyne-local`) and `raw_heap/free.rs`
/// (in `mnemosyne-heap`) perform this identical 3-step sequence — one for
/// the hot `thread_free` path and one for the branded `RawHeap::free` path.
/// Sharing the body here compiles it once per `B` instead of once per `(P,B)`
/// at each use site.
///
/// `enable_poisoning` and `poison_free_byte` must equal the policy's
/// `P::ENABLE_POISONING` and `P::POISON_FREE_BYTE`.
///
/// # Safety
///
/// `ptr` must be a live large/huge block previously returned by backend `B`.
/// The owning `*mut Segment` is stored in the pointer-sized slot directly
/// preceding the user payload (written at allocation time).
#[inline(always)]
pub unsafe fn free_large_or_huge_raw<B: mnemosyne_arena::HasSegmentPool>(
    ptr: *mut u8,
    enable_poisoning: bool,
    poison_free_byte: u8,
) {
    // SAFETY: per the caller's contract, `(ptr as *mut *mut Segment) - 1` is
    // the metadata slot written at `allocate_large_or_huge` time.
    let segment = unsafe { *((ptr as *mut *mut Segment).sub(1)) };
    if enable_poisoning {
        // SAFETY: `segment` is the live owning header; `huge_mapping_suffix_from`
        // reads only metadata within that mapping.
        let size = unsafe { (*segment).huge_mapping_suffix_from(ptr) };
        // SAFETY: `ptr` is valid for `size` bytes within the live mapping.
        unsafe { core::ptr::write_bytes(ptr, poison_free_byte, size) };
    }
    // SAFETY: `ptr`/`segment` are the matching pair for `deallocate_large_or_huge`.
    let _ = unsafe { mnemosyne_arena::deallocate_large_or_huge::<B>(ptr, segment) };
}
