use core::sync::atomic::Ordering;
use mnemosyne_core::policy::AllocPolicy;

use super::{
    CpuCacheSlot, DISABLE_CPU_CACHE, MAX_CACHED_BLOCKS, PER_CPU_CACHE, PER_CPU_CACHE_ENABLED,
    get_current_cpu_id, refresh_current_cpu_id,
};

/// Tries to allocate a block from the per-CPU cache.
///
/// Returns null when the policy uses free-list encryption (HardenedPolicy):
/// the cache stores raw unencoded pointers that are incompatible with
/// per-page XOR keys. Otherwise delegates to `try_alloc_cpu_raw` which
/// compiles once regardless of how many non-encrypting policies call this.
#[inline(always)]
pub fn try_alloc_cpu<P: AllocPolicy>(class: usize) -> *mut u8 {
    if P::ENABLE_FREE_LIST_ENCRYPTION {
        return core::ptr::null_mut();
    }
    try_alloc_cpu_raw(class)
}

/// Non-generic SSOT for the CPU-cache allocation path.
///
/// Extracted from `try_alloc_cpu<P>` so StandardPolicy and SecurePolicy — both
/// non-encrypting — share one instantiation of the ~60-line body.
#[inline(always)]
fn try_alloc_cpu_raw(class: usize) -> *mut u8 {
    if DISABLE_CPU_CACHE.load(Ordering::Relaxed) || !PER_CPU_CACHE_ENABLED.load(Ordering::Relaxed) {
        return core::ptr::null_mut();
    }

    let mut cpu_id = get_current_cpu_id();
    let mut slot: &CpuCacheSlot = &PER_CPU_CACHE.slots[cpu_id];
    let mut refreshed = false;

    for _ in 0..2 {
        let mut found_idx = None;
        let mut block_ptr = core::ptr::null_mut();

        for i in 0..MAX_CACHED_BLOCKS {
            let val = slot.blocks[class][i].load(Ordering::Relaxed);
            if !val.is_null() {
                found_idx = Some(i);
                block_ptr = val;
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

        // Strong, for the reason given on the matching exchange in
        // `try_free_cpu`: the retry budget here is for CPU migration, and a
        // spurious failure would spend it while reporting an empty cache.
        match slot.blocks[class][idx].compare_exchange(
            block_ptr,
            core::ptr::null_mut(),
            Ordering::Acquire,
            Ordering::Relaxed,
        ) {
            Ok(_) => {
                return block_ptr;
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
///
/// The cache stores raw unencoded links, so an encrypted segment block must
/// never enter it even when the freeing call uses a standard policy.
#[inline(always)]
pub fn try_free_cpu(ptr: *mut u8, class: usize, encrypted: bool) -> bool {
    if ptr.is_null() {
        return false;
    }

    if encrypted {
        return false;
    }

    if DISABLE_CPU_CACHE.load(Ordering::Relaxed) || !PER_CPU_CACHE_ENABLED.load(Ordering::Relaxed) {
        return false;
    }

    let mut cpu_id = get_current_cpu_id();
    let mut slot: &CpuCacheSlot = &PER_CPU_CACHE.slots[cpu_id];
    let mut refreshed = false;

    for _ in 0..2 {
        let mut found_idx = None;
        let mut is_double_free = false;
        for i in 0..MAX_CACHED_BLOCKS {
            let val = slot.blocks[class][i].load(Ordering::Relaxed);
            if val == ptr {
                is_double_free = true;
                break;
            }
            if val.is_null() && found_idx.is_none() {
                found_idx = Some(i);
            }
        }

        if is_double_free {
            std::process::abort();
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

        // Strong, not weak. The two-round budget in this loop exists for
        // *CPU migration* — the retry re-reads the processor id and moves
        // to the new core's slot. A weak exchange may fail spuriously with
        // the slot empty and uncontended, which spends that budget on a
        // non-event and then reports "cache unavailable" for a cache that
        // was in fact available. Miri models spurious failure deliberately
        // and caught exactly that; real LL/SC targets do it too. A strong
        // failure means the slot genuinely changed under us, which is the
        // condition the retry is written for.
        match slot.blocks[class][idx].compare_exchange(
            core::ptr::null_mut(),
            ptr,
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
