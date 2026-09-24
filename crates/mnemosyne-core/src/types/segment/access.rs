//! Ownership and current-slicing accessor methods for [`super::Segment`].
//!
//! Every accessor here takes a raw `*const Segment` / `*mut Segment` rather
//! than `&self` / `&mut self` because a shared reference retags the **whole**
//! `Segment` header. The owning thread may concurrently write other fields
//! (Miri caught exactly that against `is_current`), so the only safe form is a
//! raw-pointer field projection that touches only the one field of interest.

use super::Segment;
use crate::types::SegmentOwner;

impl Segment {
    /// Reads the cached owner-allocator pointer.
    ///
    /// Raw-pointer form and `Acquire` for the same reasons as
    /// [`Segment::owner`]: a `&self` accessor would retag the whole segment,
    /// and this read must observe the state the owner published.
    ///
    /// # Safety
    ///
    /// `segment` must point to a live segment header.
    #[inline(always)]
    pub unsafe fn owner_allocator(segment: *const Segment) -> *mut core::ffi::c_void {
        // SAFETY: caller guarantees a live header; the projection touches only
        // the atomic field.
        let field = unsafe { &raw const (*segment).ownership };
        // SAFETY: `field` addresses the initialized ownership pair.
        unsafe { (*field).allocator() }
    }

    /// Publishes the cached owner-allocator pointer.
    ///
    /// # Safety
    ///
    /// `segment` must point to a live segment header.
    #[inline(always)]
    pub unsafe fn set_owner_allocator(segment: *const Segment, allocator: *mut core::ffi::c_void) {
        // SAFETY: caller guarantees a live header.
        let field = unsafe { &raw const (*segment).ownership };
        // SAFETY: `field` addresses the initialized ownership pair.
        unsafe { (*field).set_allocator(allocator) };
    }

    /// Reads the active-slicing flag.
    ///
    /// Takes a raw pointer for the same reason as [`Segment::owner`]: a `&self`
    /// would retag the whole header and race with any concurrent write to any
    /// other field. The projection touches this one byte.
    ///
    /// # Safety
    ///
    /// `segment` must point to a live segment header, and the caller must be
    /// that segment's owning thread. This field is not synchronized; reading it
    /// from a non-owner thread is a data race.
    #[inline(always)]
    pub unsafe fn is_current(segment: *const Segment) -> bool {
        // SAFETY: caller guarantees a live header owned by this thread; the
        // projection reads only the `is_current` byte.
        let field = unsafe { &raw const (*segment).is_current };
        // SAFETY: `field` addresses the initialized flag.
        unsafe { field.read() }
    }

    /// Sets or clears the active-slicing flag.
    ///
    /// # Safety
    ///
    /// Carries [`Segment::is_current`]'s contract: live header, owning thread.
    /// Writing from a non-owner thread is a data race.
    #[inline(always)]
    pub unsafe fn set_current(segment: *mut Segment, value: bool) {
        // SAFETY: caller guarantees a live header owned by this thread; the
        // projection writes only the `is_current` byte.
        let field = unsafe { &raw mut (*segment).is_current };
        // SAFETY: `field` addresses the initialized flag.
        unsafe { field.write(value) };
    }

    /// Reads the segment's owner identity.
    ///
    /// Takes a raw pointer rather than `&self` on purpose: a reference retags
    /// the *whole* `Segment`, which races against any concurrent write to any
    /// other field (Miri caught exactly that against `is_current`). Projecting
    /// to the single atomic field touches only that field.
    ///
    /// `Acquire`: a remote thread reading this to route a cross-thread free must
    /// see the segment state published by the owner's `Release` write in
    /// [`Segment::set_owner`].
    ///
    /// # Safety
    ///
    /// `segment` must point to a live segment header.
    #[inline(always)]
    pub unsafe fn owner(segment: *const Segment) -> SegmentOwner {
        // SAFETY: caller guarantees a live header; the projection touches only
        // the `owner` field.
        let field = unsafe { &raw const (*segment).ownership };
        // SAFETY: `field` addresses the initialized ownership pair.
        unsafe { (*field).owner() }
    }

    /// Publishes the segment's owner identity.
    ///
    /// Takes a raw pointer for the same aliasing and ordering reasons as
    /// [`Segment::owner`]. The owner write is `Release`, so a remote thread
    /// that observes the identity with [`Segment::owner`] also observes the
    /// state published before the claim.
    ///
    /// # Safety
    ///
    /// `segment` must point to a live segment header owned exclusively by the
    /// caller for the duration of the write.
    #[inline(always)]
    pub unsafe fn set_owner(segment: *const Segment, owner: SegmentOwner) {
        // SAFETY: caller guarantees a live header and exclusive ownership; the
        // projection touches only the atomic owner field.
        let field = unsafe { &raw const (*segment).ownership };
        // SAFETY: `field` addresses the initialized ownership pair.
        unsafe { (*field).set_owner(owner) };
    }

    /// Clears both ownership fields atomically — sets the allocator to null and
    /// the owner token to [`SegmentOwner::NONE`].
    ///
    /// This is the **SSOT** for the 2-line "null out + clear token" sequence
    /// that appears in `reclaim_owned_segments`, `detach_and_release_segment`,
    /// and `drain_orphan_pool`. Calling both stores in sequence is correct only
    /// when the caller exclusively owns the segment (no remote thread can read
    /// a partially-cleared identity).
    ///
    /// # Safety
    ///
    /// `segment` must point to a live segment header that is exclusively owned
    /// by the caller.
    #[inline(always)]
    pub unsafe fn clear_ownership(segment: *mut Segment) {
        // SAFETY: caller guarantees exclusive ownership; both writes are to the
        // initialized ownership field via raw projections.
        unsafe {
            Self::set_owner_allocator(segment, core::ptr::null_mut());
            Self::set_owner(segment, SegmentOwner::NONE);
        }
    }
}

// ── Zero-cost page-bit iterator ───────────────────────────────────────────────

/// Iterator over the **non-zero set-bit indices** of any `u32` segment page
/// bitmask (`page_occupied_mask` or `page_linked_mask`), automatically
/// skipping bit 0 (the segment header / page-0 slot).
///
/// This replaces the repeated hand-rolled pattern:
///
/// ```text
/// let mut mask = (*seg).page_occupied_mask;   // or page_linked_mask
/// while mask != 0 {
///     let i = mask.trailing_zeros() as usize;
///     mask &= mask - 1;
///     if i == 0 { continue; }
///     // ... body using i
/// }
/// ```
///
/// The iterator is a newtype over a `u32`, so it is zero-cost in release
/// builds; the entire loop body inlines.
pub struct OccupiedPageBits {
    mask: u32,
}

impl OccupiedPageBits {
    /// Constructs the iterator from a `page_occupied_mask` or `page_linked_mask`
    /// value, clearing bit 0 so page 0 (the segment header) is never yielded.
    #[inline(always)]
    pub fn new(mask: u32) -> Self {
        // Clear bit 0: page 0 is the segment header and is never in any list.
        Self { mask: mask & !1 }
    }
}

impl Iterator for OccupiedPageBits {
    type Item = usize;

    #[inline(always)]
    fn next(&mut self) -> Option<usize> {
        if self.mask == 0 {
            return None;
        }
        let i = self.mask.trailing_zeros() as usize;
        self.mask &= self.mask - 1;
        Some(i)
    }
}
