use core::ops::Deref;
use core::sync::atomic::AtomicPtr;
use mnemosyne_core::constants::NUM_SIZE_CLASSES;
use std::boxed::Box;
use std::sync::OnceLock;

use super::state::{MAX_CACHED_BLOCKS, NUM_CPU_SLOTS};

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
    pub slots: [CpuCacheSlot; NUM_CPU_SLOTS],
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
            slots: [const { CpuCacheSlot::new() }; NUM_CPU_SLOTS],
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
    pub(super) storage: OnceLock<Box<PerCpuCache>>,
}

impl PerCpuCacheHandle {
    /// Creates an uninitialized cache handle.
    pub const fn new() -> Self {
        Self {
            storage: OnceLock::new(),
        }
    }

    #[inline]
    pub(super) fn get(&self) -> &PerCpuCache {
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
