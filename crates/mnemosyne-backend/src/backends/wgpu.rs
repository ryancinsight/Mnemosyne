//! `WgpuStagingBackend` — adapter that hooks wgpu's allocator callbacks
//! into the Mnemosyne `MemoryBackend` seam.
//!
//! Consumers register callbacks through [`register_wgpu_callbacks`]. The raw
//! storage remains private to this crate, so consumers cannot poison the
//! backend with a mismatched function pointer type. The other trait methods use
//! the trait defaults (resets/guards/decommit return `false`, no SUPPORTS_*
//! flags).

use core::ffi::c_void;
use core::sync::atomic::Ordering;
use mnemosyne_core::MemoryBackend;

/// Allocation callback installed for [`WgpuStagingBackend`].
pub type WgpuAllocateCallback = unsafe extern "C" fn(usize) -> *mut u8;

/// Deallocation callback installed for [`WgpuStagingBackend`].
pub type WgpuDeallocateCallback = unsafe extern "C" fn(*mut u8, usize) -> bool;

/// Registers the process-global WGPU staging allocation callbacks.
///
/// The callback pair is read by [`WgpuStagingBackend`] on each allocation and
/// deallocation. Re-registering replaces the previous pair atomically in
/// allocate-then-deallocate order.
///
/// # Safety
///
/// `allocate` must return either null or a pointer to at least `size` writable
/// bytes whose lifetime is valid until `deallocate` receives the same pointer
/// and size. `deallocate` must release only pointers returned by the matching
/// `allocate` callback and must not unwind across the FFI boundary.
pub unsafe fn register_wgpu_callbacks(
    allocate: WgpuAllocateCallback,
    deallocate: WgpuDeallocateCallback,
) {
    crate::WGPU_ALLOCATE_CALLBACK.store(allocate as *mut c_void, Ordering::Release);
    crate::WGPU_DEALLOCATE_CALLBACK.store(deallocate as *mut c_void, Ordering::Release);
}

#[inline]
fn registered_allocate_callback() -> Option<WgpuAllocateCallback> {
    let callback = crate::WGPU_ALLOCATE_CALLBACK.load(Ordering::Acquire);
    if callback.is_null() {
        None
    } else {
        // SAFETY: `register_wgpu_callbacks` is the only public write path for
        // this slot and stores values of exactly `WgpuAllocateCallback`.
        Some(unsafe { core::mem::transmute::<*mut c_void, WgpuAllocateCallback>(callback) })
    }
}

#[inline]
fn registered_deallocate_callback() -> Option<WgpuDeallocateCallback> {
    let callback = crate::WGPU_DEALLOCATE_CALLBACK.load(Ordering::Acquire);
    if callback.is_null() {
        None
    } else {
        // SAFETY: `register_wgpu_callbacks` is the only public write path for
        // this slot and stores values of exactly `WgpuDeallocateCallback`.
        Some(unsafe { core::mem::transmute::<*mut c_void, WgpuDeallocateCallback>(callback) })
    }
}

/// A memory backend that delegates allocation/deallocation to registered callbacks.
/// Used to hook wgpu buffer staging/allocation into Mnemosyne.
pub struct WgpuStagingBackend;

impl MemoryBackend for WgpuStagingBackend {
    const SUPPORTS_PAGE_RESET: bool = false;
    const SUPPORTS_MAKE_GUARD: bool = false;
    const SUPPORTS_DECOMMIT: bool = false;

    #[inline]
    unsafe fn allocate(size: usize) -> *mut u8 {
        if let Some(callback) = registered_allocate_callback() {
            unsafe { callback(size) }
        } else {
            core::ptr::null_mut()
        }
    }

    #[inline]
    unsafe fn deallocate(ptr: *mut u8, size: usize) -> bool {
        if let Some(callback) = registered_deallocate_callback() {
            unsafe { callback(ptr, size) }
        } else {
            false
        }
    }
}

#[cfg(test)]
mod tests {
    extern crate std;

    use core::alloc::Layout;
    use mnemosyne_core::MemoryBackend;

    use super::{WgpuStagingBackend, register_wgpu_callbacks};

    unsafe extern "C" fn test_alloc(size: usize) -> *mut u8 {
        unsafe { std::alloc::alloc(Layout::from_size_align_unchecked(size, 8)) }
    }

    unsafe extern "C" fn test_dealloc(ptr: *mut u8, size: usize) -> bool {
        unsafe {
            std::alloc::dealloc(ptr, Layout::from_size_align_unchecked(size, 8));
        }
        true
    }

    #[test]
    fn typed_callbacks_round_trip_allocation() {
        unsafe { register_wgpu_callbacks(test_alloc, test_dealloc) };

        let ptr = unsafe { WgpuStagingBackend::allocate(64) };
        assert!(!ptr.is_null());

        unsafe {
            ptr.write(0x5A);
            assert_eq!(ptr.read(), 0x5A);
        }

        assert!(unsafe { WgpuStagingBackend::deallocate(ptr, 64) });
    }
}
