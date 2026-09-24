//! Free-list encryption accessors for [`super::Segment`]: cookie derivation,
//! mode validation, and the per-page key read.
//!
//! These four methods form the **single authoritative** freelist-cookie API.
//! Every consumer — free, realloc, pop, reclaim, initialization — routes its
//! `if encrypted { keys[i] } else { 0 }` through here rather than indexing
//! `keys` inline.

use super::Segment;
use crate::abort::abort_on_corruption;
use crate::constants::PAGES_PER_SEGMENT;

/// Aborts when `segment` is null or misaligned, using `context` in the message.
///
/// SSOT for the null + alignment guard that [`Segment::cookie_for_dynamic`]
/// and [`Segment::free_list_mode_matches`] both require before any field read.
#[inline(always)]
fn assert_segment_ptr(segment: *const Segment, context: &'static str) {
    if segment.is_null() {
        abort_on_corruption(context);
    }
    if !segment
        .addr()
        .is_multiple_of(core::mem::align_of::<Segment>())
    {
        abort_on_corruption(context);
    }
}

impl Segment {
    /// Returns the free-list encryption cookie for page `page_index` under a
    /// runtime encryption flag: the per-page key when `encrypted`, else `0`.
    ///
    /// This is the single authoritative cookie accessor; the free, realloc,
    /// pop, reclaim, and initialization paths route their `if encrypted {
    /// keys[i] } else { 0 }` selection through it (or the `P`-generic
    /// [`Segment::cookie_for`]) instead of indexing `keys` inline.
    ///
    /// # Safety
    ///
    /// `self` must be this page's parent segment header and `page_index` must be
    /// a valid index into `keys` (`< PAGES_PER_SEGMENT`).
    #[inline(always)]
    pub unsafe fn cookie_for_dynamic(
        segment: *const Segment,
        encrypted: bool,
        page_index: usize,
    ) -> usize {
        assert_segment_ptr(segment, "free-list cookie: null or misaligned segment pointer");
        if page_index >= PAGES_PER_SEGMENT {
            abort_on_corruption("free-list cookie page index out of range");
        }
        // SAFETY: the guards above discharge the precondition on `segment`.
        if !unsafe { Self::free_list_mode_matches(segment, encrypted) } {
            abort_on_corruption(
                "free-list mode mismatch: raw/decode path does not match the segment",
            );
        }
        if encrypted {
            // Projected rather than reached through `&self`: this runs on the
            // cross-thread free path, where the owning thread may concurrently
            // write other segment fields. A reference retags the *whole*
            // `Segment`, which races with those writes — Miri reported exactly
            // that against `owner_allocator`. Touching only `keys` does not.
            //
            // SAFETY: the caller's contract guarantees `segment` is the valid
            // parent header and `page_index` is in range.
            let keys = unsafe { &raw const (*segment).keys };
            // SAFETY: `keys` addresses the initialized per-page key array and
            // `page_index` is in range.
            unsafe { *(*keys).get_unchecked(page_index) }
        } else {
            0
        }
    }

    /// Returns the free-list encryption cookie for page `page_index` under the
    /// compile-time policy `P`: the per-page key when `P` encrypts, else `0`.
    ///
    /// The const `P::ENABLE_FREE_LIST_ENCRYPTION` const-propagates into
    /// [`Segment::cookie_for_dynamic`], so the branch resolves at compile time.
    ///
    /// Debug builds additionally enforce that the static policy agrees with
    /// this segment's recorded mode. This remains a diagnostic guard for
    /// lower-level direct `ThreadAllocator` callers; the public thread-local
    /// selector prevents the mismatch by assigning each mode its own cache.
    /// Release builds compile the check out.
    ///
    /// # Safety
    ///
    /// Same contract as [`Segment::cookie_for_dynamic`].
    #[inline(always)]
    pub unsafe fn cookie_for<P: crate::policy::AllocPolicy>(
        segment: *const Segment,
        page_index: usize,
    ) -> usize {
        // SAFETY: caller guarantees a valid header; reading one field by
        // projection avoids retagging the whole segment.
        let recorded = unsafe { Self::free_list_encrypted(segment) };
        if recorded != P::ENABLE_FREE_LIST_ENCRYPTION {
            abort_on_corruption("free-list mode mismatch: policy vs segment (ADR 0001)");
        }
        // SAFETY: forwarded unchanged from this method's `# Safety` contract.
        unsafe { Self::cookie_for_dynamic(segment, P::ENABLE_FREE_LIST_ENCRYPTION, page_index) }
    }

    /// Returns whether the segment's free-list links match the caller's mode.
    ///
    /// # Safety
    ///
    /// `segment` must point at a live segment whose header has been
    /// initialized and published. Null and misaligned pointers abort rather
    /// than read, but a dangling pointer into freed mapping is undetectable
    /// here. Raw-pointer form for the reason the sibling accessors give: a
    /// `&Segment` would retag the whole header against the owner's concurrent
    /// writes.
    #[inline(always)]
    pub unsafe fn free_list_mode_matches(segment: *const Segment, encrypted: bool) -> bool {
        assert_segment_ptr(segment, "free-list mode check: null or misaligned segment pointer");
        // SAFETY: the guard above leaves a valid pointer.
        unsafe { Self::free_list_encrypted(segment) == encrypted }
    }

    /// Reads the segment's free-list encryption mode.
    ///
    /// Raw-pointer form for the same reason as the other accessors here: this
    /// runs on the cross-thread free path, and a `&Segment` retags the whole
    /// header, racing with the owner's concurrent writes to unrelated fields.
    /// The field itself is only written during initialization, before the
    /// segment is published, so a plain read of it is sound once the retag is
    /// avoided.
    ///
    /// # Safety
    ///
    /// `segment` must point to a live segment header.
    #[inline(always)]
    pub unsafe fn free_list_encrypted(segment: *const Segment) -> bool {
        // SAFETY: caller guarantees a live header; the projection touches only
        // this field.
        unsafe { (*segment).free_list_encrypted }
    }
}
