//! Global huge mapping pool/cache to recycle large/huge OS memory allocations.

use core::sync::atomic::{AtomicPtr, AtomicUsize, Ordering};
use mnemosyne_core::MemoryBackend;

struct HugeSlot {
    ptr: AtomicPtr<u8>,
    size: AtomicUsize,
}

impl HugeSlot {
    const fn new() -> Self {
        Self {
            ptr: AtomicPtr::new(core::ptr::null_mut()),
            size: AtomicUsize::new(0),
        }
    }
}

/// A bounded lock-free pool to retain and recycle huge mapping allocations.
pub struct HugeMappingPool {
    slots: [HugeSlot; 8],
}

impl HugeMappingPool {
    /// Creates a new empty `HugeMappingPool`.
    pub const fn new() -> Self {
        Self {
            slots: [
                HugeSlot::new(),
                HugeSlot::new(),
                HugeSlot::new(),
                HugeSlot::new(),
                HugeSlot::new(),
                HugeSlot::new(),
                HugeSlot::new(),
                HugeSlot::new(),
            ],
        }
    }

    /// Tries to claim a cached mapping of at least `requested_size`.
    ///
    /// Returns the raw pointer to the mapping and its actual allocated size.
    pub fn try_pop(&self, requested_size: usize) -> Option<(*mut u8, usize)> {
        for slot in &self.slots {
            let ptr = slot.ptr.load(Ordering::Acquire);
            if ptr.is_null() || ptr == 1 as *mut u8 {
                continue;
            }
            let size = slot.size.load(Ordering::Acquire);
            if size >= requested_size && size <= requested_size * 2 {
                // Try to claim the slot.
                if slot
                    .ptr
                    .compare_exchange(
                        ptr,
                        core::ptr::null_mut(),
                        Ordering::AcqRel,
                        Ordering::Acquire,
                    )
                    .is_ok()
                {
                    let final_size = slot.size.swap(0, Ordering::Relaxed);
                    // Double check if size is still correct/valid.
                    if final_size >= requested_size {
                        return Some((ptr, final_size));
                    } else {
                        // Restore slot if size check failed.
                        slot.ptr.store(ptr, Ordering::Release);
                    }
                }
            }
        }
        None
    }

    /// Tries to cache a huge mapping.
    ///
    /// Returns `true` if cached, `false` if the pool is full.
    pub unsafe fn try_push<B: MemoryBackend>(&self, ptr: *mut u8, size: usize) -> bool {
        for slot in &self.slots {
            if slot.ptr.load(Ordering::Relaxed).is_null() {
                // Try to lock the slot by storing DUMMY_PTR (0x1).
                if slot
                    .ptr
                    .compare_exchange(
                        core::ptr::null_mut(),
                        1 as *mut u8,
                        Ordering::AcqRel,
                        Ordering::Acquire,
                    )
                    .is_ok()
                {
                    // Lock acquired. Write size and then the real pointer.
                    slot.size.store(size, Ordering::Release);
                    slot.ptr.store(ptr, Ordering::Release);

                    // Advisory: reset the mapping contents if supported to drop physical memory commitment.
                    if B::SUPPORTS_PAGE_RESET {
                        unsafe {
                            B::page_reset(ptr, size);
                        }
                    }
                    return true;
                }
            }
        }
        false
    }
}

unsafe impl Send for HugeMappingPool {}
unsafe impl Sync for HugeMappingPool {}
