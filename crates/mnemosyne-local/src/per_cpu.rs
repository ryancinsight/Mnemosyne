//! Lock-free per-CPU L1 block caching.
//!
//! Stores block pointers in a flat atomic array behind the process-global
//! `PER_CPU_CACHE` handle, making it 100% memory-safe and UAF-free without
//! dereferencing block payload memory. The table is allocated only when the
//! cache is explicitly used.

use core::ops::Deref;
use core::sync::atomic::{AtomicBool, AtomicPtr, Ordering};
use mnemosyne_core::constants::NUM_SIZE_CLASSES;
use mnemosyne_core::policy::AllocPolicy;
use std::boxed::Box;
use std::sync::OnceLock;

const MAX_CACHED_BLOCKS: usize = 8;
const MAX_CPUS: usize = 256;

/// Whether the per-CPU block cache is consulted on the alloc/free fast
/// path.
///
/// Disabled under `cfg(test)` so tests observe the thread-local path
/// deterministically: the per-CPU cache can otherwise satisfy a request
/// from another CPU's slot and make occupancy assertions flaky.
#[cfg(test)]
pub static PER_CPU_CACHE_ENABLED: AtomicBool = AtomicBool::new(false);

/// Whether the per-CPU block cache is consulted on the alloc/free fast
/// path.
#[cfg(not(test))]
pub static PER_CPU_CACHE_ENABLED: AtomicBool = AtomicBool::new(true);

/// A lock-free block cache slot for a single CPU, protected against UAF and ABA hazards.
#[repr(align(64))]
pub struct CpuCacheSlot {
    /// Cached block pointers per size class, null meaning an empty entry.
    ///
    /// The slot is cache-line aligned so neighbouring CPUs do not contend
    /// on the same line while pushing and popping their own caches.
    pub blocks: [[AtomicPtr<u8>; MAX_CACHED_BLOCKS]; NUM_SIZE_CLASSES],
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
            blocks: [const { [const { AtomicPtr::new(core::ptr::null_mut()) }; MAX_CACHED_BLOCKS] };
                NUM_SIZE_CLASSES],
        }
    }
}

/// Global per-CPU block cache array.
#[repr(align(64))]
pub struct PerCpuCache {
    /// One cache-line-aligned slot per CPU, indexed by CPU id.
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

/// Lazily allocated handle for the global per-CPU cache.
///
/// `OnceLock<Box<_>>` keeps the process-global static to a pointer-sized
/// initialization cell. The 720,896-byte cache table is allocated only after
/// the cache is explicitly used, so disabled production backends do not carry
/// its BSS/zero-page reservation.
pub struct PerCpuCacheHandle {
    storage: OnceLock<Box<PerCpuCache>>,
}

impl PerCpuCacheHandle {
    /// Creates an uninitialized cache handle.
    pub const fn new() -> Self {
        Self {
            storage: OnceLock::new(),
        }
    }

    #[inline]
    fn get(&self) -> &PerCpuCache {
        self.storage
            .get_or_init(|| Box::new(PerCpuCache::new()))
            .as_ref()
    }
}

impl Default for PerCpuCacheHandle {
    fn default() -> Self {
        Self::new()
    }
}

impl Deref for PerCpuCacheHandle {
    type Target = PerCpuCache;

    #[inline]
    fn deref(&self) -> &Self::Target {
        self.get()
    }
}

/// The global per-CPU cache instance.
///
/// # Backend-keying invariant (latent hazard)
///
/// This is a single process-global array, **not** keyed by memory backend. The
/// slots store raw block addresses, so a block freed here under one backend and
/// re-handed out under another would cross backend ownership. `try_alloc_cpu` /
/// `try_free_cpu` only touch it when `B::ENABLE_CPU_CACHE` is `true`, and today
/// no backend sets that constant to `true` (the `MemoryBackend` trait default
/// is `false`, and neither the Unix nor Windows `DefaultBackend` overrides it),
/// so exactly one backend — none, currently — can ever populate the cache and
/// no mixing is possible.
///
/// The correctness of the shared static therefore rests on the invariant that
/// **at most one backend enables the CPU cache per process**. Keying the cache
/// per backend (an associated-type or generic buffer on `ComputeBackend`) is the
/// robust fix but is a `[minor]`-class change (new trait surface); until then this
/// invariant is documented rather than type-enforced. Any future backend that
/// sets `ENABLE_CPU_CACHE = true` alongside another such backend must first
/// introduce that per-backend keying.
pub static PER_CPU_CACHE: PerCpuCacheHandle = PerCpuCacheHandle::new();

const _: () =
    assert!(core::mem::size_of::<PerCpuCacheHandle>() < core::mem::size_of::<PerCpuCache>());

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
///
/// The `try_alloc_cpu` / `try_free_cpu` pair share the outer shape
/// (cpu-id fetch, two-round CPU-refresh retry) but their inner steps are
/// direction-specific and deliberately not factored: alloc scans for the first
/// non-empty slot and CASes it to empty under `Acquire`, while free scans for a
/// double-free plus the first empty slot and CASes it to the pointer under
/// `Release`. Extracting a shared skeleton would obscure the differing scan
/// predicate, memory ordering, and abort condition on this hot path.
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
    let mut slot = &PER_CPU_CACHE.slots[cpu_id];
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

#[cfg(test)]
mod tests {
    use super::{MAX_CPUS, PerCpuCacheHandle};

    #[test]
    fn cache_handle_allocates_storage_on_first_access() {
        let handle = PerCpuCacheHandle::new();
        assert_eq!(handle.storage.get().map(|cache| cache.slots.len()), None);

        let _cache = handle.get();

        assert_eq!(
            handle.storage.get().map(|cache| cache.slots.len()),
            Some(MAX_CPUS)
        );
    }
}
