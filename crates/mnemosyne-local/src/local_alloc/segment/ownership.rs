use crate::local_alloc::ThreadAllocator;
use core::marker::PhantomData;
use mnemosyne_arena::HasSegmentPool;
use mnemosyne_core::constants::{PAGES_PER_SEGMENT, PAGE_SIZE};
use mnemosyne_core::policy::AllocPolicy;
use mnemosyne_core::types::{Segment, SegmentOwner};

type OwnedSegmentBrand<'id, B> = fn(&'id mut ThreadAllocator<B>) -> &'id mut ThreadAllocator<B>;

/// Zero-sized permission proving exclusive allocator authority over the
/// intrusive owned-segments list for one mutation step.
pub(crate) struct OwnedSegmentToken<'id, B: HasSegmentPool> {
    _brand: PhantomData<OwnedSegmentBrand<'id, B>>,
}

impl<'id, B: HasSegmentPool> OwnedSegmentToken<'id, B> {
    #[inline(always)]
    fn new() -> Self {
        Self {
            _brand: PhantomData,
        }
    }

    /// Brands `segment` with this allocator-owned-list permission.
    ///
    /// # Safety
    ///
    /// `segment` must identify a live segment whose owned-list metadata is
    /// controlled by the allocator permission represented by this token.
    #[inline(always)]
    unsafe fn segment(&mut self, segment: *mut Segment) -> BrandedSegment<'id> {
        BrandedSegment {
            ptr: segment,
            _brand: PhantomData,
        }
    }
}

#[derive(Clone, Copy)]
struct BrandedSegment<'id> {
    ptr: *mut Segment,
    _brand: PhantomData<fn(&'id mut Segment) -> &'id mut Segment>,
}

impl BrandedSegment<'_> {
    #[inline(always)]
    fn ptr(self) -> *mut Segment {
        self.ptr
    }
}

#[inline(always)]
fn with_owned_segment_token<B: HasSegmentPool, R>(
    f: impl for<'id> FnOnce(OwnedSegmentToken<'id, B>) -> R,
) -> R {
    f(OwnedSegmentToken::new())
}

/// Prepends a branded segment to a branded intrusive owned-segments list.
///
/// # Safety
///
/// `segment` and the list rooted at `head_slot` must belong to `token`, and
/// `segment` must not already be linked into any owned-segments list.
#[inline(always)]
unsafe fn push_owned_segment_front<'id, B: HasSegmentPool>(
    token: &mut OwnedSegmentToken<'id, B>,
    head_slot: &mut *mut Segment,
    segment: BrandedSegment<'id>,
) {
    let raw_segment = segment.ptr();
    unsafe {
        (*raw_segment).prev_owned_segment = core::ptr::null_mut();
        (*raw_segment).next_owned_segment = *head_slot;
        if !(*head_slot).is_null() {
            let _head = token.segment(*head_slot);
            (**head_slot).prev_owned_segment = raw_segment;
        }
        *head_slot = raw_segment;
    }
}

/// Unlinks a branded segment from a branded intrusive owned-segments list.
///
/// # Safety
///
/// `segment` must be linked in the list rooted at `head_slot`, and its
/// neighbours must belong to the same token permission.
#[inline(always)]
unsafe fn unlink_owned_segment_from_list<'id, B: HasSegmentPool>(
    token: &mut OwnedSegmentToken<'id, B>,
    head_slot: &mut *mut Segment,
    segment: BrandedSegment<'id>,
) {
    let raw_segment = segment.ptr();
    unsafe {
        let prev = (*raw_segment).prev_owned_segment;
        let next = (*raw_segment).next_owned_segment;
        if prev.is_null() {
            *head_slot = next;
        } else {
            let _prev = token.segment(prev);
            (*prev).next_owned_segment = next;
        }
        if !next.is_null() {
            let _next = token.segment(next);
            (*next).prev_owned_segment = prev;
        }
        (*raw_segment).prev_owned_segment = core::ptr::null_mut();
        (*raw_segment).next_owned_segment = core::ptr::null_mut();
    }
}

impl<B: HasSegmentPool> ThreadAllocator<B> {
    /// Prepends `segment` to this thread's intrusive doubly-linked
    /// owned-segments list and stamps the ownership token.
    ///
    /// This is the single authoritative insertion point for the owned-segments
    /// list; both the fresh-segment and orphan-adoption paths route through it
    /// so the `prev`/`next` invariant and `owned_segment_count` stay aligned.
    ///
    /// # Safety
    ///
    /// `segment` must be a live segment owned exclusively by this allocator and
    /// must not already be linked into any owned-segments list.
    #[inline]
    pub(crate) unsafe fn push_owned_segment<P: AllocPolicy>(&mut self, segment: *mut Segment) {
        #[cfg(all(windows, target_arch = "x86_64", not(miri)))]
        {
            let tid = {
                let val: u32;
                unsafe {
                    core::arch::asm!(
                        "mov {0:e}, gs:[0x48]",
                        out(reg) val,
                        options(nostack, preserves_flags, readonly)
                    );
                }
                val
            };
            unsafe {
                (*segment).owner = SegmentOwner::from_thread_id(tid);
            }
        }
        #[cfg(any(not(all(windows, target_arch = "x86_64")), miri))]
        {
            unsafe {
                (*segment).owner = SegmentOwner::from_ptr(self as *mut ThreadAllocator<B>);
            }
        }
        with_owned_segment_token::<B, _>(|mut token| {
            let branded_segment = unsafe { token.segment(segment) };
            unsafe {
                push_owned_segment_front(&mut token, &mut self.owned_segments_head, branded_segment)
            };
        });
        self.owned_segment_count += 1;

        if P::ENABLE_FREE_LIST_ENCRYPTION {
            unsafe { self.initialize_segment_keys(segment) };
        }
    }

    /// Populates the keys array of a newly acquired segment using the thread-local seed.
    ///
    /// # Safety
    ///
    /// `segment` must point to a valid, writable `Segment`.
    #[inline]
    pub unsafe fn initialize_segment_keys(&mut self, segment: *mut Segment) {
        let seed = super::super::get_tls_seed();
        let segment_addr = segment as usize;
        unsafe {
            (*segment).free_list_encrypted = true;
            for i in 0..PAGES_PER_SEGMENT {
                (*segment).keys[i] = (segment_addr.wrapping_add(i * PAGE_SIZE)) ^ seed;
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
        with_owned_segment_token::<B, _>(|mut token| {
            let branded_segment = unsafe { token.segment(segment) };
            unsafe {
                unlink_owned_segment_from_list(
                    &mut token,
                    &mut self.owned_segments_head,
                    branded_segment,
                )
            };
        });
        debug_assert!(self.owned_segment_count > 0);
        self.owned_segment_count -= 1;
    }
}
