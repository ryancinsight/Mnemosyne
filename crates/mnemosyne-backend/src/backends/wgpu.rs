//! `WgpuStagingBackend` — adapter that hooks wgpu's allocator callbacks
//! into the Mnemosyne `MemoryBackend` seam.
//!
//! The actual callbacks live at [`crate::WGPU_ALLOCATE_CALLBACK`] and
//! [`crate::WGPU_DEALLOCATE_CALLBACK`]; wgpu registers them through the
//! public `AtomicPtr` slots, this backend transmutes them at each
//! allocator call. The other trait methods use the trait defaults
//! (resets/guards/decommit return `false`, no SUPPORTS_* flags).

use core::sync::atomic::Ordering;
use mnemosyne_core::MemoryBackend;

/// A memory backend that delegates allocation/deallocation to registered callbacks.
/// Used to hook wgpu buffer staging/allocation into Mnemosyne.
pub struct WgpuStagingBackend;

impl MemoryBackend for WgpuStagingBackend {
    const SUPPORTS_PAGE_RESET: bool = false;
    const SUPPORTS_MAKE_GUARD: bool = false;
    const SUPPORTS_DECOMMIT: bool = false;

    #[inline]
    unsafe fn allocate(size: usize) -> *mut u8 {
        let callback = crate::WGPU_ALLOCATE_CALLBACK.load(Ordering::Acquire);
        if !callback.is_null() {
            type AllocFn = unsafe extern "C" fn(usize) -> *mut u8;
            let alloc_fn: AllocFn = unsafe { core::mem::transmute(callback) };
            unsafe { alloc_fn(size) }
        } else {
            core::ptr::null_mut()
        }
    }

    #[inline]
    unsafe fn deallocate(ptr: *mut u8, size: usize) -> bool {
        let callback = crate::WGPU_DEALLOCATE_CALLBACK.load(Ordering::Acquire);
        if !callback.is_null() {
            type DeallocFn = unsafe extern "C" fn(*mut u8, usize) -> bool;
            let dealloc_fn: DeallocFn = unsafe { core::mem::transmute(callback) };
            unsafe { dealloc_fn(ptr, size) }
        } else {
            false
        }
    }
}
