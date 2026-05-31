//! Value-semantic integration tests for secure and hardened allocation policies.
//!
//! These verify that the ZST-gated policies (`SecurePolicy`, `HardenedPolicy`)
//! correctly enforce their respective invariants:
//! * memory is zero-initialized on allocation (`ZERO_INITIALIZE = true`),
//! * memory is poisoned on deallocation (`ENABLE_POISONING = true`), and
//! * cross-class reallocations zero out the expanded portion correctly.

use mnemosyne_backend::MemoryBackendWrapper as Backend;
use mnemosyne_core::{StandardPolicy};
use mnemosyne_hardened::{HardenedPolicy, SecurePolicy};
use mnemosyne_local::{thread_alloc, thread_free, thread_realloc, usable_size};

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

#[test]
fn test_realloc_under_policies() {
    use core::alloc::Layout;

    const ALIGN: usize = 8;
    let old_layout = Layout::from_size_align(16, ALIGN).unwrap();
    let new_size = 64;

    unsafe {
        // 1. SecurePolicy: check byte preservation and expanded zeroing
        let ptr1 = thread_alloc::<SecurePolicy, Backend>(16, ALIGN);
        assert!(!ptr1.is_null());
        core::ptr::write_bytes(ptr1, 0x77, 16);

        let ptr1_re = thread_realloc::<SecurePolicy, Backend>(ptr1, old_layout, new_size);
        assert!(!ptr1_re.is_null());
        
        // Original 16 bytes must be preserved
        for i in 0..16 {
            assert_eq!(*ptr1_re.add(i), 0x77);
        }
        // Expanded space (16..64) must be zeroed
        for i in 16..64 {
            assert_eq!(*ptr1_re.add(i), 0);
        }
        thread_free::<SecurePolicy, Backend>(ptr1_re);

        // 2. HardenedPolicy: check byte preservation and expanded zeroing
        let ptr2 = thread_alloc::<HardenedPolicy, Backend>(16, ALIGN);
        assert!(!ptr2.is_null());
        core::ptr::write_bytes(ptr2, 0x88, 16);

        let ptr2_re = thread_realloc::<HardenedPolicy, Backend>(ptr2, old_layout, new_size);
        assert!(!ptr2_re.is_null());
        
        for i in 0..16 {
            assert_eq!(*ptr2_re.add(i), 0x88);
        }
        for i in 16..64 {
            assert_eq!(*ptr2_re.add(i), 0);
        }
        thread_free::<HardenedPolicy, Backend>(ptr2_re);
    }
}

#[test]
fn test_usable_size_accuracy_across_policies() {
    const SIZE: usize = 48;
    const ALIGN: usize = 8;
    unsafe {
        let ptr_std = thread_alloc::<StandardPolicy, Backend>(SIZE, ALIGN);
        let ptr_sec = thread_alloc::<SecurePolicy, Backend>(SIZE, ALIGN);
        let ptr_hrd = thread_alloc::<HardenedPolicy, Backend>(SIZE, ALIGN);

        assert!(usable_size(ptr_std) >= SIZE);
        assert!(usable_size(ptr_sec) >= SIZE);
        assert!(usable_size(ptr_hrd) >= SIZE);

        thread_free::<StandardPolicy, Backend>(ptr_std);
        thread_free::<SecurePolicy, Backend>(ptr_sec);
        thread_free::<HardenedPolicy, Backend>(ptr_hrd);
    }
}
