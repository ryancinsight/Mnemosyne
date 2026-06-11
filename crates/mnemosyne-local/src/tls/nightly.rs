//! Nightly-channel TLS provider using `#[thread_local]` statics.
//!
//! Compiled only when `nightly_tls_active` cfg is set. Provides a direct
//! `#[thread_local]` fast path that bypasses `LocalAllocatorSlot`
//! lazy-initialization on hot allocation paths.

use super::traits::{TlsProvider, TlsSlotAccess};
use crate::ThreadAllocator;
use mnemosyne_arena::HasSegmentPool;

/// Unifies the nightly `#[thread_local]` compiler-backed TLS path.
pub struct NightlyTls<B, S>(core::marker::PhantomData<(B, S)>);

impl<B: HasSegmentPool, S: TlsSlotAccess<B>> TlsProvider<B> for NightlyTls<B, S> {
    const IDENTIFIER: &'static str = "NightlyTls";

    #[inline(always)]
    fn with_allocator<R>(f: impl FnOnce(&mut ThreadAllocator<B>) -> R) -> Option<R> {
        #[cfg(nightly_tls_active)]
        {
            S::get_slot_nightly(|slot| {
                S::arm_thread_exit(slot);
                slot.with_allocator(f)
            })
        }
        #[cfg(not(nightly_tls_active))]
        {
            let _ = f;
            None
        }
    }

    #[inline(always)]
    fn with_allocator_guard<R>(f: impl FnOnce(&mut ThreadAllocator<B>) -> R) -> Option<R> {
        #[cfg(nightly_tls_active)]
        {
            S::get_slot_nightly(|slot| {
                S::arm_thread_exit(slot);
                slot.with_allocator(f)
            })
        }
        #[cfg(not(nightly_tls_active))]
        {
            let _ = f;
            None
        }
    }

    #[inline(always)]
    unsafe fn with_allocator_unguarded<R>(
        f: impl FnOnce(&mut ThreadAllocator<B>) -> R,
    ) -> Option<R> {
        #[cfg(nightly_tls_active)]
        {
            S::get_slot_nightly(|slot| unsafe { slot.with_allocator_unguarded(f) })
        }
        #[cfg(not(nightly_tls_active))]
        {
            let _ = f;
            None
        }
    }

    #[inline(always)]
    fn get_allocator_ptr() -> *mut core::ffi::c_void {
        #[cfg(nightly_tls_active)]
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
        #[cfg(not(nightly_tls_active))]
        {
            core::ptr::null_mut()
        }
    }

    #[inline(always)]
    fn get_allocator_ptr_raw() -> *mut core::ffi::c_void {
        #[cfg(nightly_tls_active)]
        {
            S::get_quick_allocator_ptr()
        }
        #[cfg(not(nightly_tls_active))]
        {
            core::ptr::null_mut()
        }
    }
}
