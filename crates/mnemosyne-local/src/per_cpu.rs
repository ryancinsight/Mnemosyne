//! Lock-free per-CPU L1 block caching with ABA protection.

use core::sync::atomic::{AtomicUsize, AtomicU8, AtomicBool, Ordering};
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
pub struct CpuCacheSlot {
    pub heads: [AtomicUsize; NUM_SIZE_CLASSES],
    pub counts: [AtomicU8; NUM_SIZE_CLASSES],
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
pub struct PerCpuCache {
    pub slots: [CpuCacheSlot; MAX_CPUS],
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
    #[cfg(target_os = "linux")]
    {
        unsafe {
            let mut cpu_val = 0u32;
            let mut ret: isize;
            core::arch::asm!(
                "syscall",
                in("rax") 309isize, // __NR_getcpu
                in("rdi") &mut cpu_val as *mut u32,
                in("rsi") core::ptr::null_mut::<u32>(),
                in("rdx") core::ptr::null_mut::<u8>(),
                lateout("rax") ret,
                lateout("rcx") _,
                lateout("r11") _,
                options(nostack, preserves_flags)
            );
            if ret == 0 {
                cpu_val as usize % MAX_CPUS
            } else {
                0
            }
        }
    }
    #[cfg(windows)]
    {
        unsafe {
            extern "system" {
                fn GetCurrentProcessorNumber() -> u32;
            }
            GetCurrentProcessorNumber() as usize % MAX_CPUS
        }
    }
    #[cfg(not(any(target_os = "linux", target_os = "windows")))]
    {
        0
    }
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

    let cpu_id = current_cpu_id();
    let slot = &PER_CPU_CACHE.slots[cpu_id];
    
    loop {
        let count = slot.counts[class].load(Ordering::Relaxed);
        if count == 0 {
            return core::ptr::null_mut();
        }
        
        let packed_head = slot.heads[class].load(Ordering::Acquire);
        let (head_ptr, count_val) = unpack_ptr(packed_head);
        if head_ptr.is_null() {
            return core::ptr::null_mut();
        }
        
        // Safety: head_ptr points to a valid cached block.
        let next_ptr = unsafe { *(head_ptr as *mut *mut u8) };
        let new_packed = pack_ptr(next_ptr, count_val + 1);
        
        if slot.heads[class].compare_exchange_weak(
            packed_head,
            new_packed,
            Ordering::Release,
            Ordering::Acquire,
        ).is_ok() {
            slot.counts[class].fetch_sub(1, Ordering::Release);
            return head_ptr;
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

    let cpu_id = current_cpu_id();
    let slot = &PER_CPU_CACHE.slots[cpu_id];
    
    loop {
        let count = slot.counts[class].load(Ordering::Relaxed);
        if count >= MAX_CACHED_BLOCKS {
            return false;
        }
        
        let packed_head = slot.heads[class].load(Ordering::Acquire);
        let (head_ptr, count_val) = unpack_ptr(packed_head);
        
        // Safety: ptr is a valid free block.
        unsafe {
            *(ptr as *mut *mut u8) = head_ptr;
        }
        let new_packed = pack_ptr(ptr, count_val + 1);
        
        if slot.heads[class].compare_exchange_weak(
            packed_head,
            new_packed,
            Ordering::Release,
            Ordering::Acquire,
        ).is_ok() {
            slot.counts[class].fetch_add(1, Ordering::Release);
            return true;
        }
    }
}
