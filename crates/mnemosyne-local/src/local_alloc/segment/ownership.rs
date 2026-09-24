use crate::local_alloc::ThreadAllocator;
use mnemosyne_arena::HasSegmentPool;
use mnemosyne_core::constants::{PAGE_SIZE, PAGES_PER_SEGMENT};
use mnemosyne_core::policy::AllocPolicy;
use mnemosyne_core::types::{Segment, SegmentOwner};

// ── Non-generic raw implementations ──────────────────────────────────────────
//
// OwnedSegmentToken<'id,B>/BrandedSegment<'id> were GhostCell-style ZSTs —
// token.segment(ptr).ptr() == ptr at runtime. Exclusive access is already
// enforced by &mut ThreadAllocator<B> at every call site; the raw helpers
// compile once and are called directly.

/// Non-generic core for prepending a segment to an intrusive owned-segments list.
///
/// # Safety
///
/// `raw_segment` must be exclusively owned and not yet linked in any list.
/// The list rooted at `head_slot` must be exclusively accessible.
#[inline(always)]
pub(crate) unsafe fn push_owned_segment_front_raw(
    raw_segment: *mut Segment,
    head_slot: &mut *mut Segment,
) {
    // SAFETY: `raw_segment` is exclusively owned per the caller's contract;
    // reading and writing its link fields is unaliased.
    unsafe {
        (*raw_segment).prev_owned_segment = core::ptr::null_mut();
        (*raw_segment).next_owned_segment = *head_slot;
        if !(*head_slot).is_null() {
            // SAFETY: `*head_slot` is in the caller-owned list; setting its
            // back-pointer is an exclusive write within the same list.
            (**head_slot).prev_owned_segment = raw_segment;
        }
        *head_slot = raw_segment;
    }
}

/// Non-generic core for unlinking a segment from an intrusive owned-segments list.
///
/// # Safety
///
/// `raw_segment` must be exclusively owned and currently linked in the list
/// rooted at `head_slot`. All neighbour segments must be exclusively accessible.
#[inline(always)]
pub(crate) unsafe fn unlink_owned_segment_from_list_raw(
    raw_segment: *mut Segment,
    head_slot: &mut *mut Segment,
) {
    // SAFETY: `raw_segment` is exclusively owned per the caller's contract;
    // reading its link fields is unaliased.
    unsafe {
        let prev = (*raw_segment).prev_owned_segment;
        let next = (*raw_segment).next_owned_segment;
        if prev.is_null() {
            *head_slot = next;
        } else {
            // SAFETY: `prev` is a live segment in the caller-owned list.
            (*prev).next_owned_segment = next;
        }
        if !next.is_null() {
            // SAFETY: `next` is a live segment in the caller-owned list.
            (*next).prev_owned_segment = prev;
        }
        (*raw_segment).prev_owned_segment = core::ptr::null_mut();
        (*raw_segment).next_owned_segment = core::ptr::null_mut();
    }
}

impl<B: HasSegmentPool> ThreadAllocator<B> {
    /// Non-`<P>` core of [`push_owned_segment`]: stamps ownership metadata
    /// and links `segment` into the owned-segments list.
    ///
    /// Extracted so the P-generic wrapper compiles to a 2-line thin shell; the
    /// ~25-line body is shared across all policy instantiations.
    ///
    /// # Safety
    ///
    /// Same contract as `push_owned_segment`.
    #[inline(always)]
    unsafe fn push_owned_segment_core(&mut self, segment: *mut Segment) {
        let allocator_ptr = (self as *mut ThreadAllocator<B>).cast::<core::ffi::c_void>();
        // SAFETY: `segment` is the live caller-passed segment; `self` is the
        // owning allocator. The writes are not aliased by any concurrent accessor.
        unsafe { Segment::set_owner_allocator(segment, allocator_ptr) };
        // Owner encoding differs by platform: Windows x86-64 encodes a thread
        // ID so `resolve_owner_slot` can detect same-thread cross-policy access;
        // all other targets encode the allocator pointer directly.
        #[cfg(all(windows, target_arch = "x86_64", not(miri)))]
        unsafe {
            Segment::set_owner(
                segment,
                SegmentOwner::from_thread_id(mnemosyne_core::types::current_thread_id()),
            )
        };
        #[cfg(not(all(windows, target_arch = "x86_64", not(miri))))]
        unsafe {
            Segment::set_owner(
                segment,
                SegmentOwner::from_ptr(self as *mut ThreadAllocator<B>),
            )
        };
        // SAFETY: `segment` is the just-acquired live segment, exclusively owned
        // by `self`; &mut self proves exclusive access to owned_segments_head.
        unsafe { push_owned_segment_front_raw(segment, &mut self.owned_segments_head) };
        self.owned_segment_count += 1;
    }

    /// Prepends `segment` to this thread's intrusive doubly-linked
    /// owned-segments list and stamps the ownership token.
    ///
    /// # Safety
    ///
    /// `segment` must be a live segment owned exclusively by this allocator and
    /// must not already be linked into any owned-segments list.
    #[cfg(test)]
    #[inline]
    pub(crate) unsafe fn push_owned_segment<P: AllocPolicy>(&mut self, segment: *mut Segment) {
        // SAFETY: forwarded.
        unsafe { self.push_owned_segment_dynamic(segment, P::ENABLE_FREE_LIST_ENCRYPTION) }
    }

    /// Non-generic variant of `push_owned_segment`.
    ///
    /// Used by the `alloc_cold_raw` → `get_new_page_dynamic` cold path which
    /// passes runtime bools rather than policy type parameters.
    ///
    /// # Safety
    ///
    /// Same contract as `push_owned_segment`.
    #[inline]
    pub(crate) unsafe fn push_owned_segment_dynamic(
        &mut self,
        segment: *mut Segment,
        enable_encryption: bool,
    ) {
        // SAFETY: forwarded — same contract.
        unsafe { self.push_owned_segment_core(segment) };

        if enable_encryption {
            // SAFETY: same as `push_owned_segment<P>` — live segment, owned
            // exclusively, no live chains when `free_list_encrypted` is false.
            let already_keyed = unsafe { (*segment).free_list_encrypted };
            if !already_keyed {
                unsafe { self.initialize_segment_keys(segment) };
            }
        }
    }

    /// Populates the keys array of a newly acquired segment using the thread-local seed.
    ///
    /// # Safety
    ///
    /// `segment` must point to a valid, writable `Segment` that holds no live
    /// encoded free-list chains and is not visible to any other thread (no
    /// remote frees in flight): the key writes are non-atomic and invalidate
    /// every link encoded with the previous keys, so calling this on a segment
    /// with live allocations corrupts its free lists and races concurrent
    /// `AtomicFreeList` key reads.
    #[inline]
    pub unsafe fn initialize_segment_keys(&mut self, segment: *mut Segment) {
        let seed = super::super::get_tls_seed();
        let process_key = super::super::get_process_key();
        let segment_addr = segment as usize;
        // SAFETY: `segment` is a valid, writable `Segment` (caller contract)
        // owned exclusively by this allocator. `i` ranges over
        // `0..PAGES_PER_SEGMENT`, the exact length of the `keys` array, so every
        // `keys[i]` write is in-bounds and unaliased.
        unsafe {
            (*segment).free_list_encrypted = true;
            for i in 0..PAGES_PER_SEGMENT {
                // Triangular XOR: page address ^ per-thread seed ^ process key.
                // All three components are required to reconstruct a key:
                //   - page address: observable (same thread) but predictable with ASLR bypass
                //   - per-thread seed: random per thread, potentially disclosable
                //   - process key: random per process, independent of thread seeds
                (*segment).keys[i] =
                    (segment_addr.wrapping_add(i * PAGE_SIZE)) ^ seed ^ process_key;
            }
        }
    }

    /// Unlinks a segment from the owned segments list in O(1).
    ///
    /// The list is intrusive and doubly linked, so the segment's own
    /// `prev_owned_segment`/`next_owned_segment` pointers locate both
    /// neighbours directly; no linear search for the predecessor is required.
    /// Both link fields are cleared so the detached segment carries no stale
    /// pointers into the list.
    #[inline]
    pub(crate) unsafe fn unlink_owned_segment(&mut self, segment: *mut Segment) {
        // SAFETY: `segment` is exclusively owned by this thread (it is in the
        // owned list); &mut self proves exclusive access to owned_segments_head.
        unsafe { unlink_owned_segment_from_list_raw(segment, &mut self.owned_segments_head) };
        debug_assert!(self.owned_segment_count > 0);
        self.owned_segment_count -= 1;
    }
}
