//! Lock-free per-CPU L1 block caching with ABA protection.

use core::sync::atomic::{AtomicBool, AtomicU8, AtomicUsize, Ordering};
use mnemosyne_core::constants::NUM_SIZE_CLASSES;
use mnemosyne_core::policy::AllocPolicy;

const MAX_CACHED_BLOCKS: u8 = 8;
const MAX_CPUS: usize = 256;

const PACKED_PTR_BITS: u32 = 48;
const PTR_MASK: usize = (1usize << PACKED_PTR_BITS) - 1;
const COUNT_WRAP_MASK: usize = (1usize << (usize::BITS - PACKED_PTR_BITS)) - 1;

#[cfg(test)]
pub static PER_CPU_CACHE_ENABLED: AtomicBool = AtomicBool::new(false);

#[cfg(not(test))]
pub static PER_CPU_CACHE_ENABLED: AtomicBool = AtomicBool::new(true);

/// A lock-free block cache slot for a single CPU, protected against ABA hazards.
#[repr(align(64))]
pub struct CpuCacheSlot {
    pub heads: [AtomicUsize; NUM_SIZE_CLASSES],
    pub counts: [AtomicU8; NUM_SIZE_CLASSES],
}

impl Default for CpuCacheSlot {
    #[inline]
    fn default() -> Self {
        Self::new()
    }
}

impl CpuCacheSlot {
    /// Creates a new empty `CpuCacheSlot`.
    pub const fn new() -> Self {
        Self {
            heads: [const { AtomicUsize::new(0) }; NUM_SIZE_CLASSES],
            counts: [const { AtomicU8::new(0) }; NUM_SIZE_CLASSES],
        }
    }
}

/// Global per-CPU block cache array.
#[repr(align(64))]
pub struct PerCpuCache {
    pub slots: [CpuCacheSlot; MAX_CPUS],
}

impl Default for PerCpuCache {
    #[inline]
    fn default() -> Self {
        Self::new()
    }
}

impl PerCpuCache {
    /// Creates a new empty `PerCpuCache`.
    pub const fn new() -> Self {
        Self {
            slots: [const { CpuCacheSlot::new() }; MAX_CPUS],
        }
    }
}

/// The global per-CPU cache instance.
pub static PER_CPU_CACHE: PerCpuCache = PerCpuCache::new();

static DISABLE_CPU_CACHE: AtomicBool = AtomicBool::new(false);

/// Dynamically disables the per-CPU cache.
pub fn disable_cpu_cache() {
    DISABLE_CPU_CACHE.store(true, Ordering::Relaxed);
}

/// Dynamically enables the per-CPU cache.
pub fn enable_cpu_cache() {
    DISABLE_CPU_CACHE.store(false, Ordering::Relaxed);
}

/// Returns the current CPU ID.
#[inline]
pub fn current_cpu_id() -> usize {
    themis::current_processor().map_or(0, |cpu| cpu as usize) % MAX_CPUS
}

#[cfg(nightly_tls_active)]
#[thread_local]
static mut CACHED_CPU_ID: usize = usize::MAX;

#[cfg(not(nightly_tls_active))]
std::thread_local! {
    static CACHED_CPU_ID: core::cell::Cell<usize> = const { core::cell::Cell::new(usize::MAX) };
}

/// Returns the cached CPU ID, or queries the OS and caches it if uninitialized.
#[inline(always)]
pub fn get_current_cpu_id() -> usize {
    #[cfg(nightly_tls_active)]
    unsafe {
        let val = CACHED_CPU_ID;
        if val != usize::MAX {
            val
        } else {
            let actual = current_cpu_id();
            CACHED_CPU_ID = actual;
            actual
        }
    }
    #[cfg(not(nightly_tls_active))]
    {
        CACHED_CPU_ID.with(|cell| {
            let val = cell.get();
            if val != usize::MAX {
                val
            } else {
                let actual = current_cpu_id();
                cell.set(actual);
                actual
            }
        })
    }
}

/// Force-refreshes the cached CPU ID from the OS.
#[inline(always)]
pub fn refresh_current_cpu_id() -> usize {
    let actual = current_cpu_id();
    #[cfg(nightly_tls_active)]
    unsafe {
        CACHED_CPU_ID = actual;
    }
    #[cfg(not(nightly_tls_active))]
    {
        CACHED_CPU_ID.with(|cell| cell.set(actual));
    }
    actual
}

#[inline]
fn pack_ptr(ptr: *mut u8, count: usize) -> usize {
    let ptr_val = ptr as usize;
    debug_assert_eq!(ptr_val & !PTR_MASK, 0);
    let count_val = count & COUNT_WRAP_MASK;
    ptr_val | (count_val << PACKED_PTR_BITS)
}

#[inline]
fn unpack_ptr(packed: usize) -> (*mut u8, usize) {
    let ptr_val = packed & PTR_MASK;
    let count_val = packed >> PACKED_PTR_BITS;
    (ptr_val as *mut u8, count_val)
}

/// Tries to allocate a block from the per-CPU cache.
#[inline(always)]
pub fn try_alloc_cpu<P: AllocPolicy>(class: usize) -> *mut u8 {
    if DISABLE_CPU_CACHE.load(Ordering::Relaxed) || !PER_CPU_CACHE_ENABLED.load(Ordering::Relaxed) {
        return core::ptr::null_mut();
    }

    let mut cpu_id = get_current_cpu_id();
    let mut slot = &PER_CPU_CACHE.slots[cpu_id];
    let mut refreshed = false;

    loop {
        let count = slot.counts[class].load(Ordering::Relaxed);
        if count == 0 {
            if !refreshed {
                let new_cpu_id = refresh_current_cpu_id();
                if new_cpu_id != cpu_id {
                    cpu_id = new_cpu_id;
                    slot = &PER_CPU_CACHE.slots[cpu_id];
                    refreshed = true;
                    continue;
                }
            }
            return core::ptr::null_mut();
        }

        let packed_head = slot.heads[class].load(Ordering::Acquire);
        let (head_ptr, count_val) = unpack_ptr(packed_head);
        if head_ptr.is_null() {
            if !refreshed {
                let new_cpu_id = refresh_current_cpu_id();
                if new_cpu_id != cpu_id {
                    cpu_id = new_cpu_id;
                    slot = &PER_CPU_CACHE.slots[cpu_id];
                    refreshed = true;
                    continue;
                }
            }
            return core::ptr::null_mut();
        }

        // Safety: head_ptr points to a valid cached block.
        let next_ptr = unsafe { *(head_ptr as *mut *mut u8) };
        let new_packed = pack_ptr(next_ptr, count_val + 1);

        match slot.heads[class].compare_exchange_weak(
            packed_head,
            new_packed,
            Ordering::Release,
            Ordering::Acquire,
        ) {
            Ok(_) => {
                slot.counts[class].fetch_sub(1, Ordering::Release);
                return head_ptr;
            }
            Err(_) => {
                if !refreshed {
                    let new_cpu_id = refresh_current_cpu_id();
                    if new_cpu_id != cpu_id {
                        cpu_id = new_cpu_id;
                        slot = &PER_CPU_CACHE.slots[cpu_id];
                        refreshed = true;
                    }
                }
            }
        }
    }
}

/// Tries to free a block back to the per-CPU cache.
#[inline(always)]
pub fn try_free_cpu<P: AllocPolicy>(ptr: *mut u8, class: usize) -> bool {
    if ptr.is_null() {
        return false;
    }

    if DISABLE_CPU_CACHE.load(Ordering::Relaxed) || !PER_CPU_CACHE_ENABLED.load(Ordering::Relaxed) {
        return false;
    }

    let mut cpu_id = get_current_cpu_id();
    let mut slot = &PER_CPU_CACHE.slots[cpu_id];
    let mut refreshed = false;

    loop {
        let count = slot.counts[class].load(Ordering::Relaxed);
        if count >= MAX_CACHED_BLOCKS {
            if !refreshed {
                let new_cpu_id = refresh_current_cpu_id();
                if new_cpu_id != cpu_id {
                    cpu_id = new_cpu_id;
                    slot = &PER_CPU_CACHE.slots[cpu_id];
                    refreshed = true;
                    continue;
                }
            }
            return false;
        }

        let packed_head = slot.heads[class].load(Ordering::Acquire);
        let (head_ptr, count_val) = unpack_ptr(packed_head);

        // Safety: ptr is a valid free block.
        unsafe {
            *(ptr as *mut *mut u8) = head_ptr;
        }
        let new_packed = pack_ptr(ptr, count_val + 1);

        match slot.heads[class].compare_exchange_weak(
            packed_head,
            new_packed,
            Ordering::Release,
            Ordering::Acquire,
        ) {
            Ok(_) => {
                slot.counts[class].fetch_add(1, Ordering::Release);
                return true;
            }
            Err(_) => {
                if !refreshed {
                    let new_cpu_id = refresh_current_cpu_id();
                    if new_cpu_id != cpu_id {
                        cpu_id = new_cpu_id;
                        slot = &PER_CPU_CACHE.slots[cpu_id];
                        refreshed = true;
                    }
                }
            }
        }
    }
}
