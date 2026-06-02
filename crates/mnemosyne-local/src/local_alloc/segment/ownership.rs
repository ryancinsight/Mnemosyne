use crate::local_alloc::ThreadAllocator;
use mnemosyne_arena::HasSegmentPool;
use mnemosyne_core::constants::{PAGES_PER_SEGMENT, PAGE_SIZE};
use mnemosyne_core::policy::AllocPolicy;
use mnemosyne_core::types::{Segment, SegmentOwner};

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
        unsafe {
            #[cfg(all(windows, target_arch = "x86_64"))]
            {
                let tid = {
                    let val: u32;
                    core::arch::asm!(
                        "mov {0:e}, gs:[0x48]",
                        out(reg) val,
                        options(nostack, preserves_flags, readonly)
                    );
                    val
                };
                (*segment).owner = SegmentOwner::from_thread_id(tid);
            }
            #[cfg(not(all(windows, target_arch = "x86_64")))]
            {
                (*segment).owner = SegmentOwner::from_ptr(self as *mut ThreadAllocator<B>);
            }
            (*segment).prev_owned_segment = core::ptr::null_mut();
            (*segment).next_owned_segment = self.owned_segments_head;
            if !self.owned_segments_head.is_null() {
                (*self.owned_segments_head).prev_owned_segment = segment;
            }
            self.owned_segments_head = segment;
            self.owned_segment_count += 1;

            if P::ENABLE_FREE_LIST_ENCRYPTION {
                self.initialize_segment_keys(segment);
            }
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
        unsafe {
            let prev = (*segment).prev_owned_segment;
            let next = (*segment).next_owned_segment;
            if prev.is_null() {
                self.owned_segments_head = next;
            } else {
                (*prev).next_owned_segment = next;
            }
            if !next.is_null() {
                (*next).prev_owned_segment = prev;
            }
            (*segment).prev_owned_segment = core::ptr::null_mut();
            (*segment).next_owned_segment = core::ptr::null_mut();
        }
        debug_assert!(self.owned_segment_count > 0);
        self.owned_segment_count -= 1;
    }
}
