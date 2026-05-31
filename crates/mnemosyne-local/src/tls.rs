//! Highly optimized, monomorphized Thread Local Storage (TLS) provider implementations.

use crate::{LocalAllocatorSlot, ThreadAllocator};
use core::sync::atomic::{AtomicU32, Ordering};
use mnemosyne_arena::HasSegmentPool;

/// Trait providing access to the raw thread-local slot and exit registration hook.
pub trait TlsSlotAccess<B: HasSegmentPool>: 'static {
    /// Executes the closure with a reference to the standard thread-local allocator slot.
    fn get_slot_standard<R>(f: impl FnOnce(&LocalAllocatorSlot<B>) -> R) -> R;

    /// Executes the closure with a reference to the thread-local pointer cache cell.
    fn get_cached_cell<R>(f: impl FnOnce(&core::cell::Cell<*mut core::ffi::c_void>) -> R) -> R;

    /// Arms the thread-exit reclamation sentinel for the given slot.
    fn arm_thread_exit(slot: &LocalAllocatorSlot<B>);

    /// Returns the static atomic holding the platform-native OS TLS key.
    fn get_os_tls_key() -> &'static AtomicU32;

    /// Executes the closure with a reference to the nightly `#[thread_local]` static.
    #[cfg(feature = "nightly_tls")]
    fn get_slot_nightly<R>(f: impl FnOnce(&LocalAllocatorSlot<B>) -> R) -> R;

    /// Returns the raw thread-local pointer to the allocator cache without closure overhead.
    #[cfg(feature = "nightly_tls")]
    fn get_quick_allocator_ptr() -> *mut core::ffi::c_void;

    /// Sets the raw thread-local pointer to the allocator cache.
    #[cfg(feature = "nightly_tls")]
    fn set_quick_allocator_ptr(ptr: *mut core::ffi::c_void);
}

/// Monomorphized interface for accessing thread-local allocator caches.
pub trait TlsProvider<B: HasSegmentPool>: 'static {
    /// Friendly identifier for diagnostics and benchmarking.
    const IDENTIFIER: &'static str;

    /// Runs `f` with a mutable reference to the thread-local allocator cache.
    fn with_allocator<R>(f: impl FnOnce(&mut ThreadAllocator<B>) -> R) -> Option<R>;

    /// Runs `f` with the thread-local allocator cache, arming the re-entrancy guard.
    fn with_allocator_guard<R>(f: impl FnOnce(&mut ThreadAllocator<B>) -> R) -> Option<R>;

    /// Runs `f` with the thread-local allocator cache without arming the re-entrancy guard.
    ///
    /// # Safety
    ///
    /// `f` must not re-enter the allocator.
    unsafe fn with_allocator_unguarded<R>(
        f: impl FnOnce(&mut ThreadAllocator<B>) -> R,
    ) -> Option<R>;

    /// Returns the raw pointer to the thread-local allocator cache.
    fn get_allocator_ptr() -> *mut core::ffi::c_void;

    /// Returns the raw pointer to the thread-local allocator cache without triggering lazy initialization.
    fn get_allocator_ptr_raw() -> *mut core::ffi::c_void;
}

/// Portable TLS provider using direct standard `std::thread_local!` lookups.
pub struct StandardTls<B, S>(core::marker::PhantomData<(B, S)>);

impl<B: HasSegmentPool, S: TlsSlotAccess<B>> TlsProvider<B> for StandardTls<B, S> {
    const IDENTIFIER: &'static str = "StandardTls";

    #[inline(always)]
    fn with_allocator<R>(f: impl FnOnce(&mut ThreadAllocator<B>) -> R) -> Option<R> {
        S::get_slot_standard(|slot| {
            S::arm_thread_exit(slot);
            slot.with_allocator(f)
        })
    }

    #[inline(always)]
    fn with_allocator_guard<R>(f: impl FnOnce(&mut ThreadAllocator<B>) -> R) -> Option<R> {
        S::get_slot_standard(|slot| {
            S::arm_thread_exit(slot);
            slot.with_allocator(f)
        })
    }

    #[inline(always)]
    unsafe fn with_allocator_unguarded<R>(
        f: impl FnOnce(&mut ThreadAllocator<B>) -> R,
    ) -> Option<R> {
        S::get_slot_standard(|slot| unsafe { slot.with_allocator_unguarded(f) })
    }

    #[inline(always)]
    fn get_allocator_ptr() -> *mut core::ffi::c_void {
        S::get_slot_standard(|slot| slot.allocator_ptr())
    }

    #[inline(always)]
    fn get_allocator_ptr_raw() -> *mut core::ffi::c_void {
        S::get_slot_standard(|slot| slot.allocator_ptr())
    }
}

/// Portable TLS provider that caches the raw slot pointer in a standard `thread_local!` `Cell`.
/// Bypasses lazy-initialization overhead of the full allocator slot on subsequent accesses.
pub struct CachedCellTls<B, S>(core::marker::PhantomData<(B, S)>);

impl<B: HasSegmentPool, S: TlsSlotAccess<B>> TlsProvider<B> for CachedCellTls<B, S> {
    const IDENTIFIER: &'static str = "CachedCellTls";

    #[inline(always)]
    fn with_allocator<R>(f: impl FnOnce(&mut ThreadAllocator<B>) -> R) -> Option<R> {
        let ptr = S::get_cached_cell(|cell| cell.get());
        if !ptr.is_null() {
            let alloc = unsafe { &mut *(ptr as *mut ThreadAllocator<B>) };
            if alloc.is_allocating {
                return None;
            }
            alloc.is_allocating = true;
            let result = f(alloc);
            alloc.is_allocating = false;
            Some(result)
        } else {
            S::get_slot_standard(|slot| {
                let alloc_ptr = slot.allocator_ptr();
                S::get_cached_cell(|cell| cell.set(alloc_ptr));
                S::arm_thread_exit(slot);
                slot.with_allocator(f)
            })
        }
    }

    #[inline(always)]
    fn with_allocator_guard<R>(f: impl FnOnce(&mut ThreadAllocator<B>) -> R) -> Option<R> {
        let ptr = S::get_cached_cell(|cell| cell.get());
        if !ptr.is_null() {
            let alloc = unsafe { &mut *(ptr as *mut ThreadAllocator<B>) };
            if alloc.is_allocating {
                return None;
            }
            alloc.is_allocating = true;
            let result = f(alloc);
            alloc.is_allocating = false;
            Some(result)
        } else {
            S::get_slot_standard(|slot| {
                let alloc_ptr = slot.allocator_ptr();
                S::get_cached_cell(|cell| cell.set(alloc_ptr));
                S::arm_thread_exit(slot);
                slot.with_allocator(f)
            })
        }
    }

    #[inline(always)]
    unsafe fn with_allocator_unguarded<R>(
        f: impl FnOnce(&mut ThreadAllocator<B>) -> R,
    ) -> Option<R> {
        let ptr = S::get_cached_cell(|cell| cell.get());
        if !ptr.is_null() {
            let alloc = unsafe { &mut *(ptr as *mut ThreadAllocator<B>) };
            if alloc.is_allocating {
                return None;
            }
            Some(f(alloc))
        } else {
            S::get_slot_standard(|slot| {
                let alloc_ptr = slot.allocator_ptr();
                S::get_cached_cell(|cell| cell.set(alloc_ptr));
                S::arm_thread_exit(slot);
                unsafe { slot.with_allocator_unguarded(f) }
            })
        }
    }

    #[inline(always)]
    fn get_allocator_ptr() -> *mut core::ffi::c_void {
        let ptr = S::get_cached_cell(|cell| cell.get());
        if !ptr.is_null() {
            ptr
        } else {
            S::get_slot_standard(|slot| {
                let alloc_ptr = slot.allocator_ptr();
                S::get_cached_cell(|cell| cell.set(alloc_ptr));
                alloc_ptr
            })
        }
    }

    #[inline(always)]
    fn get_allocator_ptr_raw() -> *mut core::ffi::c_void {
        S::get_cached_cell(|cell| cell.get())
    }
}

/// Platform-native TLS provider using OS-level slots (`TlsGetValue` / `pthread_getspecific`).
pub struct NativeOsTls<B, S>(core::marker::PhantomData<(B, S)>);

impl<B: HasSegmentPool, S: TlsSlotAccess<B>> TlsProvider<B> for NativeOsTls<B, S> {
    const IDENTIFIER: &'static str = "NativeOsTls";

    #[inline(always)]
    fn with_allocator<R>(f: impl FnOnce(&mut ThreadAllocator<B>) -> R) -> Option<R> {
        let key = get_os_tls_key(S::get_os_tls_key());
        let ptr = get_os_tls_value(key);
        if !ptr.is_null() {
            let alloc = unsafe { &mut *(ptr as *mut ThreadAllocator<B>) };
            if alloc.is_allocating {
                return None;
            }
            alloc.is_allocating = true;
            let result = f(alloc);
            alloc.is_allocating = false;
            Some(result)
        } else {
            S::get_slot_standard(|slot| {
                let alloc_ptr = slot.allocator_ptr();
                set_os_tls_value(key, alloc_ptr);
                S::arm_thread_exit(slot);
                slot.with_allocator(f)
            })
        }
    }

    #[inline(always)]
    fn with_allocator_guard<R>(f: impl FnOnce(&mut ThreadAllocator<B>) -> R) -> Option<R> {
        let key = get_os_tls_key(S::get_os_tls_key());
        let ptr = get_os_tls_value(key);
        if !ptr.is_null() {
            let alloc = unsafe { &mut *(ptr as *mut ThreadAllocator<B>) };
            if alloc.is_allocating {
                return None;
            }
            alloc.is_allocating = true;
            let result = f(alloc);
            alloc.is_allocating = false;
            Some(result)
        } else {
            S::get_slot_standard(|slot| {
                let alloc_ptr = slot.allocator_ptr();
                set_os_tls_value(key, alloc_ptr);
                S::arm_thread_exit(slot);
                slot.with_allocator(f)
            })
        }
    }

    #[inline(always)]
    unsafe fn with_allocator_unguarded<R>(
        f: impl FnOnce(&mut ThreadAllocator<B>) -> R,
    ) -> Option<R> {
        let key = get_os_tls_key(S::get_os_tls_key());
        let ptr = get_os_tls_value(key);
        if !ptr.is_null() {
            let alloc = unsafe { &mut *(ptr as *mut ThreadAllocator<B>) };
            if alloc.is_allocating {
                return None;
            }
            Some(f(alloc))
        } else {
            S::get_slot_standard(|slot| {
                let alloc_ptr = slot.allocator_ptr();
                set_os_tls_value(key, alloc_ptr);
                S::arm_thread_exit(slot);
                unsafe { slot.with_allocator_unguarded(f) }
            })
        }
    }

    #[inline(always)]
    fn get_allocator_ptr() -> *mut core::ffi::c_void {
        let key = get_os_tls_key(S::get_os_tls_key());
        let ptr = get_os_tls_value(key);
        if !ptr.is_null() {
            ptr
        } else {
            S::get_slot_standard(|slot| {
                let alloc_ptr = slot.allocator_ptr();
                set_os_tls_value(key, alloc_ptr);
                alloc_ptr
            })
        }
    }

    #[inline(always)]
    fn get_allocator_ptr_raw() -> *mut core::ffi::c_void {
        let key = get_os_tls_key(S::get_os_tls_key());
        get_os_tls_value(key)
    }
}

/// Ultra-low latency TLS provider using direct TEB array indexing via inline assembly on Windows x86_64.
/// Falls back to `NativeOsTls` on other architectures.
pub struct AsmTls<B, S>(core::marker::PhantomData<(B, S)>);

impl<B: HasSegmentPool, S: TlsSlotAccess<B>> TlsProvider<B> for AsmTls<B, S> {
    const IDENTIFIER: &'static str = "AsmTls";

    #[inline(always)]
    fn with_allocator<R>(f: impl FnOnce(&mut ThreadAllocator<B>) -> R) -> Option<R> {
        #[cfg(all(windows, target_arch = "x86_64"))]
        {
            let key = get_os_tls_key(S::get_os_tls_key());
            let ptr = unsafe { get_teb_tls_slot(key) };
            if !ptr.is_null() {
                let alloc = unsafe { &mut *(ptr as *mut ThreadAllocator<B>) };
                if alloc.is_allocating {
                    return None;
                }
                alloc.is_allocating = true;
                let result = f(alloc);
                alloc.is_allocating = false;
                Some(result)
            } else {
                S::get_slot_standard(|slot| {
                    let alloc_ptr = slot.allocator_ptr();
                    unsafe { set_teb_tls_slot(key, alloc_ptr) };
                    S::arm_thread_exit(slot);
                    slot.with_allocator(f)
                })
            }
        }
        #[cfg(not(all(windows, target_arch = "x86_64")))]
        {
            <NativeOsTls<B, S> as TlsProvider<B>>::with_allocator(f)
        }
    }

    #[inline(always)]
    fn with_allocator_guard<R>(f: impl FnOnce(&mut ThreadAllocator<B>) -> R) -> Option<R> {
        #[cfg(all(windows, target_arch = "x86_64"))]
        {
            let key = get_os_tls_key(S::get_os_tls_key());
            let ptr = unsafe { get_teb_tls_slot(key) };
            if !ptr.is_null() {
                let alloc = unsafe { &mut *(ptr as *mut ThreadAllocator<B>) };
                if alloc.is_allocating {
                    return None;
                }
                alloc.is_allocating = true;
                let result = f(alloc);
                alloc.is_allocating = false;
                Some(result)
            } else {
                S::get_slot_standard(|slot| {
                    let alloc_ptr = slot.allocator_ptr();
                    unsafe { set_teb_tls_slot(key, alloc_ptr) };
                    S::arm_thread_exit(slot);
                    slot.with_allocator(f)
                })
            }
        }
        #[cfg(not(all(windows, target_arch = "x86_64")))]
        {
            <NativeOsTls<B, S> as TlsProvider<B>>::with_allocator_guard(f)
        }
    }

    #[inline(always)]
    unsafe fn with_allocator_unguarded<R>(
        f: impl FnOnce(&mut ThreadAllocator<B>) -> R,
    ) -> Option<R> {
        #[cfg(all(windows, target_arch = "x86_64"))]
        {
            let key = get_os_tls_key(S::get_os_tls_key());
            let ptr = unsafe { get_teb_tls_slot(key) };
            if !ptr.is_null() {
                let alloc = unsafe { &mut *(ptr as *mut ThreadAllocator<B>) };
                if alloc.is_allocating {
                    return None;
                }
                Some(f(alloc))
            } else {
                S::get_slot_standard(|slot| {
                    let alloc_ptr = slot.allocator_ptr();
                    unsafe { set_teb_tls_slot(key, alloc_ptr) };
                    S::arm_thread_exit(slot);
                    unsafe { slot.with_allocator_unguarded(f) }
                })
            }
        }
        #[cfg(not(all(windows, target_arch = "x86_64")))]
        {
            unsafe { <NativeOsTls<B, S> as TlsProvider<B>>::with_allocator_unguarded(f) }
        }
    }

    #[inline(always)]
    fn get_allocator_ptr() -> *mut core::ffi::c_void {
        #[cfg(all(windows, target_arch = "x86_64"))]
        {
            let key = get_os_tls_key(S::get_os_tls_key());
            let ptr = unsafe { get_teb_tls_slot(key) };
            if !ptr.is_null() {
                ptr
            } else {
                S::get_slot_standard(|slot| {
                    let alloc_ptr = slot.allocator_ptr();
                    unsafe { set_teb_tls_slot(key, alloc_ptr) };
                    alloc_ptr
                })
            }
        }
        #[cfg(not(all(windows, target_arch = "x86_64")))]
        {
            <NativeOsTls<B, S> as TlsProvider<B>>::get_allocator_ptr()
        }
    }

    #[inline(always)]
    fn get_allocator_ptr_raw() -> *mut core::ffi::c_void {
        #[cfg(all(windows, target_arch = "x86_64"))]
        {
            let key = get_os_tls_key(S::get_os_tls_key());
            unsafe { get_teb_tls_slot(key) }
        }
        #[cfg(not(all(windows, target_arch = "x86_64")))]
        {
            <NativeOsTls<B, S> as TlsProvider<B>>::get_allocator_ptr_raw()
        }
    }
}

/// Unifies the nightly `#[thread_local]` compiler-backed TLS path.
pub struct NightlyTls<B, S>(core::marker::PhantomData<(B, S)>);

impl<B: HasSegmentPool, S: TlsSlotAccess<B>> TlsProvider<B> for NightlyTls<B, S> {
    const IDENTIFIER: &'static str = "NightlyTls";

    #[inline(always)]
    fn with_allocator<R>(f: impl FnOnce(&mut ThreadAllocator<B>) -> R) -> Option<R> {
        #[cfg(feature = "nightly_tls")]
        {
            S::get_slot_nightly(|slot| {
                S::arm_thread_exit(slot);
                slot.with_allocator(f)
            })
        }
        #[cfg(not(feature = "nightly_tls"))]
        {
            let _ = f;
            unreachable!("NightlyTls is only available under nightly_tls feature gate");
        }
    }

    #[inline(always)]
    fn with_allocator_guard<R>(f: impl FnOnce(&mut ThreadAllocator<B>) -> R) -> Option<R> {
        #[cfg(feature = "nightly_tls")]
        {
            S::get_slot_nightly(|slot| {
                S::arm_thread_exit(slot);
                slot.with_allocator(f)
            })
        }
        #[cfg(not(feature = "nightly_tls"))]
        {
            let _ = f;
            unreachable!("NightlyTls is only available under nightly_tls feature gate");
        }
    }

    #[inline(always)]
    unsafe fn with_allocator_unguarded<R>(
        f: impl FnOnce(&mut ThreadAllocator<B>) -> R,
    ) -> Option<R> {
        #[cfg(feature = "nightly_tls")]
        {
            S::get_slot_nightly(|slot| unsafe { slot.with_allocator_unguarded(f) })
        }
        #[cfg(not(feature = "nightly_tls"))]
        {
            let _ = f;
            unreachable!("NightlyTls is only available under nightly_tls feature gate");
        }
    }

    #[inline(always)]
    fn get_allocator_ptr() -> *mut core::ffi::c_void {
        #[cfg(feature = "nightly_tls")]
        {
            let ptr = S::get_quick_allocator_ptr();
            if !ptr.is_null() {
                ptr
            } else {
                S::get_slot_nightly(|slot| {
                    S::arm_thread_exit(slot);
                    let alloc_ptr = slot.allocator_ptr();
                    S::set_quick_allocator_ptr(alloc_ptr);
                    alloc_ptr
                })
            }
        }
        #[cfg(not(feature = "nightly_tls"))]
        {
            unreachable!("NightlyTls is only available under nightly_tls feature gate");
        }
    }

    #[inline(always)]
    fn get_allocator_ptr_raw() -> *mut core::ffi::c_void {
        #[cfg(feature = "nightly_tls")]
        {
            S::get_quick_allocator_ptr()
        }
        #[cfg(not(feature = "nightly_tls"))]
        {
            unreachable!("NightlyTls is only available under nightly_tls feature gate");
        }
    }
}

// --- OS TLS Helpers ---

#[inline(always)]
fn get_os_tls_key(atomic_key: &AtomicU32) -> u32 {
    let mut key = atomic_key.load(Ordering::Acquire);
    if key == u32::MAX {
        key = init_os_tls_key(atomic_key);
    }
    key
}

#[cold]
#[inline(never)]
fn init_os_tls_key(atomic_key: &AtomicU32) -> u32 {
    unsafe {
        #[cfg(windows)]
        {
            extern "system" {
                fn TlsAlloc() -> u32;
                fn TlsFree(dwTlsIndex: u32) -> i32;
            }
            let key = TlsAlloc();
            if key == u32::MAX {
                panic!("Failed to allocate Win32 TLS index");
            }
            match atomic_key.compare_exchange(u32::MAX, key, Ordering::AcqRel, Ordering::Acquire) {
                Ok(_) => key,
                Err(existing) => {
                    TlsFree(key);
                    existing
                }
            }
        }
        #[cfg(not(windows))]
        {
            extern "C" {
                fn pthread_key_create(
                    key: *mut u32,
                    destructor: Option<unsafe extern "C" fn(*mut core::ffi::c_void)>,
                ) -> i32;
                fn pthread_key_delete(key: u32) -> i32;
            }
            let mut key = 0u32;
            let res = pthread_key_create(&mut key, None);
            if res != 0 {
                panic!("Failed to create pthread TLS key");
            }
            match atomic_key.compare_exchange(u32::MAX, key, Ordering::AcqRel, Ordering::Acquire) {
                Ok(_) => key,
                Err(existing) => {
                    pthread_key_delete(key);
                    existing
                }
            }
        }
    }
}

#[inline(always)]
fn get_os_tls_value(key: u32) -> *mut core::ffi::c_void {
    unsafe {
        #[cfg(windows)]
        {
            extern "system" {
                fn TlsGetValue(dwTlsIndex: u32) -> *mut core::ffi::c_void;
            }
            TlsGetValue(key)
        }
        #[cfg(not(windows))]
        {
            extern "C" {
                fn pthread_getspecific(key: u32) -> *mut core::ffi::c_void;
            }
            pthread_getspecific(key)
        }
    }
}

#[inline(always)]
fn set_os_tls_value(key: u32, value: *mut core::ffi::c_void) {
    unsafe {
        #[cfg(windows)]
        {
            extern "system" {
                fn TlsSetValue(dwTlsIndex: u32, lpTlsValue: *mut core::ffi::c_void) -> i32;
            }
            TlsSetValue(key, value);
        }
        #[cfg(not(windows))]
        {
            extern "C" {
                fn pthread_setspecific(key: u32, value: *const core::ffi::c_void) -> i32;
            }
            pthread_setspecific(key, value);
        }
    }
}

#[cfg(all(windows, target_arch = "x86_64"))]
#[inline(always)]
unsafe fn get_teb_tls_slot(index: u32) -> *mut core::ffi::c_void {
    if index < 64 {
        let val: *mut core::ffi::c_void;
        core::arch::asm!(
            "mov {}, gs:[0x1480 + {} * 8]",
            out(reg) val,
            in(reg) index as usize,
            options(nostack, preserves_flags, readonly)
        );
        val
    } else {
        let teb: *mut u8;
        core::arch::asm!(
            "mov {}, gs:[0x30]",
            out(reg) teb,
            options(nostack, preserves_flags, readonly)
        );
        let expansion_slots = *(teb.add(0x1780) as *mut *mut *mut core::ffi::c_void);
        if expansion_slots.is_null() {
            core::ptr::null_mut()
        } else {
            *expansion_slots.add(index as usize - 64)
        }
    }
}

#[cfg(all(windows, target_arch = "x86_64"))]
#[inline(always)]
unsafe fn set_teb_tls_slot(index: u32, value: *mut core::ffi::c_void) {
    if index < 64 {
        core::arch::asm!(
            "mov gs:[0x1480 + {} * 8], {}",
            in(reg) index as usize,
            in(reg) value,
            options(nostack, preserves_flags)
        );
    } else {
        let teb: *mut u8;
        core::arch::asm!(
            "mov {}, gs:[0x30]",
            out(reg) teb,
            options(nostack, preserves_flags, readonly)
        );
        let expansion_slots = *(teb.add(0x1780) as *mut *mut *mut core::ffi::c_void);
        if !expansion_slots.is_null() {
            *expansion_slots.add(index as usize - 64) = value;
        }
    }
}
