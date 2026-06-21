//! Lock-free per-CPU L1 block caching.
//!
//! Stores block pointers in a flat atomic array inside static global memory
//! (`PER_CPU_CACHE`), making it 100% memory-safe and UAF-free without dereferencing
//! block payload memory.

use core::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use mnemosyne_core::constants::NUM_SIZE_CLASSES;
use mnemosyne_core::policy::AllocPolicy;

const MAX_CACHED_BLOCKS: usize = 8;
const MAX_CPUS: usize = 256;

#[cfg(test)]
pub static PER_CPU_CACHE_ENABLED: AtomicBool = AtomicBool::new(false);

#[cfg(not(test))]
pub static PER_CPU_CACHE_ENABLED: AtomicBool = AtomicBool::new(true);

/// A lock-free block cache slot for a single CPU, protected against UAF and ABA hazards.
#[repr(align(64))]
pub struct CpuCacheSlot {
    pub blocks: [[AtomicUsize; MAX_CACHED_BLOCKS]; NUM_SIZE_CLASSES],
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
            blocks: [const { [const { AtomicUsize::new(0) }; MAX_CACHED_BLOCKS] }; NUM_SIZE_CLASSES],
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

melinoe::thread_cached! {
    /// Per-thread cached CPU id (melinoe `thread_cached!` SSOT; replaces the
    /// crate-local TLS pair and its `usize::MAX` sentinel — uninitialized is
    /// now a real `Option` state).
    mod cached_cpu_id: usize;
}

/// Returns the cached CPU ID, or queries the OS and caches it if uninitialized.
#[inline(always)]
pub fn get_current_cpu_id() -> usize {
    cached_cpu_id::get_or_init(current_cpu_id)
}

/// Force-refreshes the cached CPU ID from the OS.
#[inline(always)]
pub fn refresh_current_cpu_id() -> usize {
    let actual = current_cpu_id();
    cached_cpu_id::set(actual);
    actual
}

/// Tries to allocate a block from the per-CPU cache.
#[inline(always)]
pub fn try_alloc_cpu<P: AllocPolicy>(class: usize) -> *mut u8 {
    if P::ENABLE_FREE_LIST_ENCRYPTION {
        return core::ptr::null_mut();
    }

    if DISABLE_CPU_CACHE.load(Ordering::Relaxed) || !PER_CPU_CACHE_ENABLED.load(Ordering::Relaxed) {
        return core::ptr::null_mut();
    }

    let mut cpu_id = get_current_cpu_id();
    let mut slot = &PER_CPU_CACHE.slots[cpu_id];
    let mut refreshed = false;

    for _ in 0..2 {
        let mut found_idx = None;
        let mut block_ptr_val = 0;

        for i in 0..MAX_CACHED_BLOCKS {
            let val = slot.blocks[class][i].load(Ordering::Acquire);
            if val != 0 {
                found_idx = Some(i);
                block_ptr_val = val;
                break;
            }
        }

        let Some(idx) = found_idx else {
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
        };

        match slot.blocks[class][idx].compare_exchange_weak(
            block_ptr_val,
            0,
            Ordering::Acquire,
            Ordering::Relaxed,
        ) {
            Ok(_) => {
                return block_ptr_val as *mut u8;
            }
            Err(_) => {
                if !refreshed {
                    let new_cpu_id = refresh_current_cpu_id();
                    if new_cpu_id != cpu_id {
                        cpu_id = new_cpu_id;
                        slot = &PER_CPU_CACHE.slots[cpu_id];
                    }
                    refreshed = true;
                } else {
                    break;
                }
            }
        }
    }
    core::ptr::null_mut()
}

/// Tries to free a block back to the per-CPU cache.
#[inline(always)]
pub fn try_free_cpu<P: AllocPolicy>(ptr: *mut u8, class: usize) -> bool {
    if ptr.is_null() {
        return false;
    }

    if P::ENABLE_FREE_LIST_ENCRYPTION {
        return false;
    }

    if DISABLE_CPU_CACHE.load(Ordering::Relaxed) || !PER_CPU_CACHE_ENABLED.load(Ordering::Relaxed) {
        return false;
    }

    let mut cpu_id = get_current_cpu_id();
    let mut slot = &PER_CPU_CACHE.slots[cpu_id];
    let mut refreshed = false;

    for _ in 0..2 {
        let mut found_idx = None;
        for i in 0..MAX_CACHED_BLOCKS {
            let val = slot.blocks[class][i].load(Ordering::Relaxed);
            if val == 0 {
                found_idx = Some(i);
                break;
            }
        }

        let Some(idx) = found_idx else {
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
        };

        match slot.blocks[class][idx].compare_exchange_weak(
            0,
            ptr as usize,
            Ordering::Release,
            Ordering::Relaxed,
        ) {
            Ok(_) => {
                return true;
            }
            Err(_) => {
                if !refreshed {
                    let new_cpu_id = refresh_current_cpu_id();
                    if new_cpu_id != cpu_id {
                        cpu_id = new_cpu_id;
                        slot = &PER_CPU_CACHE.slots[cpu_id];
                    }
                    refreshed = true;
                } else {
                    break;
                }
            }
        }
    }
    false
}
