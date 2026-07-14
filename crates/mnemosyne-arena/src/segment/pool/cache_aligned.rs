use core::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use mnemosyne_core::types::Segment;

/// Cache-line aligned tagged atomic segment pointer used by segment-pool heads.
///
/// On 64-bit targets this packs the segment address into the low 48 bits and a
/// wrapping mutation tag into the high bits. The tag changes on every successful
/// push/pop CAS, preventing stale-head ABA from installing an obsolete
/// `next_free_segment` link.
#[repr(align(64))]
pub(crate) struct CacheAlignedAtomicPtr {
    value: AtomicUsize,
}

/// Cache-line-isolated lock protecting intrusive segment-head lifetimes.
///
/// A tagged pointer prevents stale compare-exchange success, but it cannot keep
/// a removed mapping alive while another thread dereferences an observed head.
/// Pool operations therefore hold this lock from head observation through link
/// access; detach releases it only after the old head is unreachable.
#[repr(align(64))]
pub(crate) struct CacheAlignedSegmentLock {
    held: AtomicBool,
}

impl CacheAlignedSegmentLock {
    /// Pause attempts before yielding the hardware thread. Pool critical
    /// sections contain only a head/link mutation, so 64 attempts cover the
    /// expected handoff without monopolizing a preempted core.
    const SPINS_BEFORE_YIELD: usize = 64;

    #[inline(always)]
    pub(crate) const fn new() -> Self {
        Self {
            held: AtomicBool::new(false),
        }
    }

    #[inline]
    pub(crate) fn lock(&self) -> SegmentLockGuard<'_> {
        let mut spins = 0usize;
        loop {
            if self
                .held
                .compare_exchange_weak(false, true, Ordering::Acquire, Ordering::Relaxed)
                .is_ok()
            {
                return SegmentLockGuard { lock: self };
            }
            while self.held.load(Ordering::Relaxed) {
                core::hint::spin_loop();
                spins += 1;
                if spins == Self::SPINS_BEFORE_YIELD {
                    // An allocator-internal lock cannot park through an
                    // allocation-backed primitive without risking recursion.
                    // Hosted builds yield after the bounded pause window;
                    // pure no_std targets restart the bounded hardware-pause
                    // window because no scheduler contract exists there.
                    #[cfg(any(feature = "std", test))]
                    std::thread::yield_now();
                    spins = 0;
                }
            }
        }
    }
}

/// RAII release for [`CacheAlignedSegmentLock`].
pub(crate) struct SegmentLockGuard<'a> {
    lock: &'a CacheAlignedSegmentLock,
}

impl Drop for SegmentLockGuard<'_> {
    #[inline(always)]
    fn drop(&mut self) {
        // Release publishes every protected head/link mutation to the next
        // successful Acquire lock operation.
        self.lock.held.store(false, Ordering::Release);
    }
}

impl CacheAlignedAtomicPtr {
    #[cfg(target_pointer_width = "64")]
    const PACKED_PTR_BITS: u32 = 48;
    #[cfg(not(target_pointer_width = "64"))]
    const PACKED_PTR_BITS: u32 = usize::BITS;

    #[cfg(target_pointer_width = "64")]
    const PTR_MASK: usize = (1usize << Self::PACKED_PTR_BITS) - 1;
    #[cfg(not(target_pointer_width = "64"))]
    const PTR_MASK: usize = usize::MAX;

    #[cfg(target_pointer_width = "64")]
    const TAG_MASK: usize = (1usize << (usize::BITS - Self::PACKED_PTR_BITS)) - 1;
    #[cfg(not(target_pointer_width = "64"))]
    const TAG_MASK: usize = 0;

    /// Creates a new empty head: the null pointer packed with tag 0.
    #[inline(always)]
    pub(crate) const fn new() -> Self {
        Self {
            value: AtomicUsize::new(0),
        }
    }

    #[inline(always)]
    pub(crate) fn load(&self, order: core::sync::atomic::Ordering) -> usize {
        self.value.load(order)
    }

    #[inline(always)]
    pub(crate) fn ptr(state: usize) -> *mut Segment {
        core::ptr::with_exposed_provenance_mut::<Segment>(state & Self::PTR_MASK)
    }

    #[inline(always)]
    pub(crate) fn tagged_successor(ptr: *mut Segment, current: usize) -> usize {
        let addr = ptr.expose_provenance();
        if (addr & !Self::PTR_MASK) != 0 {
            #[cfg(any(feature = "std", test))]
            {
                std::process::abort();
            }
            #[cfg(not(any(feature = "std", test)))]
            {
                panic!("Segment address does not fit in packed huge-pool head");
            }
        }

        #[cfg(target_pointer_width = "64")]
        {
            let tag = (((current >> Self::PACKED_PTR_BITS) + 1) & Self::TAG_MASK)
                << Self::PACKED_PTR_BITS;
            tag | addr
        }
        #[cfg(not(target_pointer_width = "64"))]
        {
            addr
        }
    }

    #[inline(always)]
    pub(crate) fn compare_exchange_weak(
        &self,
        current: usize,
        next: usize,
        success: core::sync::atomic::Ordering,
        failure: core::sync::atomic::Ordering,
    ) -> Result<usize, usize> {
        self.value
            .compare_exchange_weak(current, next, success, failure)
    }

    #[inline(always)]
    pub(crate) fn swap_null(&self, order: core::sync::atomic::Ordering) -> usize {
        self.value.swap(0, order)
    }
}

/// Cache-line aligned atomic counter used by segment-pool metadata.
#[repr(align(64))]
pub(crate) struct CacheAlignedAtomicUsize {
    pub(crate) value: AtomicUsize,
}

impl CacheAlignedAtomicUsize {
    #[inline(always)]
    pub(crate) const fn new(val: usize) -> Self {
        Self {
            value: AtomicUsize::new(val),
        }
    }
}
