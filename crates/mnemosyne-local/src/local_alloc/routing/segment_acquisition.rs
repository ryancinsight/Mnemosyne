//! Policy-compatible segment acquisition for the cold allocation path.
//!
//! [`acquire_policy_compatible_segment`] is the terminal stage of the cold
//! route: it pops segments from the pools/OS until one policy `P` can own
//! arrives, deferring policy-incompatible orphans back to the orphan pool.

use mnemosyne_arena::{AcquiredSegment, HasSegmentPool, acquire_segment};
use mnemosyne_core::types::Segment;

/// Pops segments from the pools/OS until one that policy `P` can own arrives,
/// returning policy-incompatible orphans to the orphan pool.
///
/// An orphan's live free chains are encoded under the segment's recorded
/// `free_list_encrypted` mode with the per-page keys already in its header,
/// while the owner-side allocation hot paths (`pop_block`, page initialization)
/// select encryption statically from `P`; local free paths use the recorded
/// mode. A thread may therefore adopt only an orphan whose
/// recorded mode matches `P::ENABLE_FREE_LIST_ENCRYPTION`; a mismatched orphan
/// is deferred on a local intrusive chain and pushed back to the orphan pool
/// for a matching-policy thread once a usable segment is found. Fresh and
/// pool-reinitialized segments (`free_list_encrypted == false`, zero live
/// allocations) are always usable: `push_owned_segment` keys them for `P`
/// before any chain is encoded.
///
/// Termination: each loop iteration either consumes one finite-pool segment
/// (a free segment ends the loop; each deferred orphan shrinks the orphan
/// pool) or reaches the OS path, which yields a fresh segment or `None`.
///
/// # Safety
///
/// Same contract as [`acquire_segment`]: the global pools must contain valid,
/// initialized `Segment`s. The returned segment (if any) is exclusively owned
/// by the caller, and an [`AcquiredSegment::Orphan`] must be adopted with its
/// live pages intact.
///
/// `enable_encryption` must equal `P::ENABLE_FREE_LIST_ENCRYPTION` for the
/// policy `P` that will own the returned segment; it is passed as a plain
/// `bool` so this function compiles once per `B` rather than once per `(P, B)`.
#[inline(never)]
pub(super) unsafe fn acquire_policy_compatible_segment<B: HasSegmentPool>(
    enable_encryption: bool,
) -> Option<AcquiredSegment> {
    let mut deferred: *mut Segment = core::ptr::null_mut();
    let chosen = loop {
        // SAFETY: this function's contract passes on `acquire_segment`'s: the
        // pools hold valid, initialized segments. Of the caller duty it adds,
        // this loop keeps its half — an orphan is either returned for the
        // caller to adopt with its live pages intact, or threaded onto
        // `deferred` and pushed back to the orphan pool below — and never
        // reaches `deallocate_segment` or the OS.
        let Some(acquired) = (unsafe { acquire_segment::<B>() }) else {
            break None;
        };
        let AcquiredSegment::Orphan(seg_ptr) = acquired else {
            break Some(acquired);
        };
        // SAFETY: `seg_ptr` is the initialized, exclusively-owned orphan just
        // returned by `acquire_segment`; `free_list_encrypted` is its recorded
        // chain-encoding mode.
        if unsafe { (*seg_ptr).free_list_encrypted } != enable_encryption {
            // SAFETY: the segment is exclusively owned after the pop, so its
            // `next_free_segment` link is free to thread the deferral chain.
            unsafe {
                (*seg_ptr)
                    .next_free_segment
                    .store(deferred, core::sync::atomic::Ordering::Relaxed);
            }
            deferred = seg_ptr;
            continue;
        }
        break Some(acquired);
    };
    while !deferred.is_null() {
        // SAFETY: `deferred` walks the exclusively-owned deferral chain built
        // above; each node is a valid orphan whose link is cleared before the
        // pool takes ownership back.
        unsafe {
            let next = (*deferred)
                .next_free_segment
                .load(core::sync::atomic::Ordering::Relaxed);
            (*deferred)
                .next_free_segment
                .store(core::ptr::null_mut(), core::sync::atomic::Ordering::Relaxed);
            B::global_orphan_pool().push_unbounded(deferred);
            deferred = next;
        }
    }
    chosen
}
