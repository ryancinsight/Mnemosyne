//! Value-semantic integration tests for secure and hardened allocation policies.
//!
//! These verify that the ZST-gated policies (`SecurePolicy`, `HardenedPolicy`)
//! correctly enforce their respective invariants:
//! * memory is zero-initialized on allocation (`ZERO_INITIALIZE = true`),
//! * memory is poisoned on deallocation (`ENABLE_POISONING = true`), and
//! * cross-class reallocations zero out the expanded portion correctly.
//!
//! Encryption-mode discipline (ADR 0001): the public thread allocator now
//! selects TLS state by backend and `ENABLE_FREE_LIST_ENCRYPTION`. Owner-side
//! links still use the segment's recorded mode, so Standard/Hardened calls may
//! interleave safely. The mixed-mode value-semantic test below exercises both
//! directions; unencrypted policies (`StandardPolicy`, `SecurePolicy`, custom
//! zero/poison policies) continue to share the standard slot.

use mnemosyne_backend::MemoryBackendWrapper as Backend;
use mnemosyne_core::policy::{HardenedPolicy, SecurePolicy};
use mnemosyne_local::{thread_alloc, thread_free};

macro_rules! impl_unencrypted_policy_slot {
    ($policy:ty) => {
        impl mnemosyne_local::tls_slot::PolicySlotSelection<Backend> for $policy {
            #[inline(always)]
            fn with_allocator<R>(
                f: impl FnOnce(&mut mnemosyne_local::ThreadAllocator<Backend>) -> R,
            ) -> Option<R> {
                <StandardPolicy as mnemosyne_local::tls_slot::PolicySlotSelection<Backend>>::with_allocator(f)
            }

            #[inline(always)]
            unsafe fn with_allocator_unguarded<R>(
                f: impl FnOnce(&mut mnemosyne_local::ThreadAllocator<Backend>) -> R,
            ) -> Option<R> {
                unsafe {
                    <StandardPolicy as mnemosyne_local::tls_slot::PolicySlotSelection<Backend>>::with_allocator_unguarded(f)
                }
            }

            #[inline(always)]
            fn get_allocator_ptr() -> *mut core::ffi::c_void {
                <StandardPolicy as mnemosyne_local::tls_slot::PolicySlotSelection<Backend>>::get_allocator_ptr()
            }

            #[inline(always)]
            fn get_allocator_ptr_raw() -> *mut core::ffi::c_void {
                <StandardPolicy as mnemosyne_local::tls_slot::PolicySlotSelection<Backend>>::get_allocator_ptr_raw()
            }
        }
    };
}

#[test]
fn test_secure_policy_zeroing() {
    const SIZE: usize = 32;
    const ALIGN: usize = 8;
    unsafe {
        let ptr1 = thread_alloc::<SecurePolicy, Backend>(SIZE, ALIGN);
        assert!(!ptr1.is_null());

        // Verify it is zero-initialized
        for i in 0..SIZE {
            assert_eq!(*ptr1.add(i), 0);
        }

        // Write a sentinel value
        core::ptr::write_bytes(ptr1, 0xAA, SIZE);
        thread_free::<SecurePolicy, Backend>(ptr1);

        // Allocate a second block of the same size class.
        // Even if the allocator reuses the same block, it must be zero-initialized.
        let ptr2 = thread_alloc::<SecurePolicy, Backend>(SIZE, ALIGN);
        assert!(!ptr2.is_null());
        for i in 0..SIZE {
            assert_eq!(*ptr2.add(i), 0);
        }
        thread_free::<SecurePolicy, Backend>(ptr2);
    }
}

#[test]
fn test_hardened_policy_round_trip() {
    const SIZE: usize = 64;
    const ALIGN: usize = 8;
    unsafe {
        let mut ptrs = [core::ptr::null_mut::<u8>(); 16];
        for slot in ptrs.iter_mut() {
            let p = thread_alloc::<HardenedPolicy, Backend>(SIZE, ALIGN);
            assert!(!p.is_null());

            // HardenedPolicy also enforces zero initialization
            for j in 0..SIZE {
                assert_eq!(*p.add(j), 0);
            }
            core::ptr::write_bytes(p, 0xBB, SIZE);
            *slot = p;
        }

        // Deallocate all to ensure free-list operations succeed with encryption enabled.
        for &p in &ptrs {
            thread_free::<HardenedPolicy, Backend>(p);
        }
    }
}

mod encoding;
mod realloc;
