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

    /// Publishes a new owner identity for this segment.
    ///
    /// `Release`: pairs with [`Segment::owner`]'s `Acquire` so everything the
    /// owner wrote before claiming or orphaning the segment is visible to the
    /// remote thread that observes the new identity. Raw-pointer form for the
    /// same whole-struct-retag reason as the reader.
    ///
    /// # Safety
    ///
    /// `segment` must point to a live segment header.
    #[inline(always)]
    pub unsafe fn set_owner(segment: *const Segment, owner: SegmentOwner) {
        // SAFETY: caller guarantees a live header.
        let field = unsafe { &raw const (*segment).ownership };
        // SAFETY: `field` addresses the initialized ownership pair.
        unsafe { (*field).set_owner(owner) };
    }
}
