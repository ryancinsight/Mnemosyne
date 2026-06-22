//! Value-semantic integration tests for secure and hardened allocation policies.
//!
//! These verify that the ZST-gated policies (`SecurePolicy`, `HardenedPolicy`)
//! correctly enforce their respective invariants:
//! * memory is zero-initialized on allocation (`ZERO_INITIALIZE = true`),
//! * memory is poisoned on deallocation (`ENABLE_POISONING = true`), and
//! * cross-class reallocations zero out the expanded portion correctly.

use mnemosyne_backend::MemoryBackendWrapper as Backend;
use mnemosyne_core::StandardPolicy;
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
    let old_layout = Layout::from_size_align(16, ALIGN)
        .expect("16-byte allocation with 8-byte alignment is a valid Layout");
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

#[test]
fn test_randomized_allocation_policy() {
    const SIZE: usize = 16;
    const ALIGN: usize = 8;

    // 1. Run StandardPolicy check in a separate thread
    let std_consecutive = std::thread::spawn(move || unsafe {
        let mut std_ptrs = [core::ptr::null_mut::<u8>(); 5];
        for slot in &mut std_ptrs {
            *slot = thread_alloc::<StandardPolicy, Backend>(SIZE, ALIGN);
            assert!(!slot.is_null());
        }

        let mut consecutive = true;
        for i in 0..4 {
            let diff = (std_ptrs[i + 1] as isize - std_ptrs[i] as isize).abs();
            if diff != SIZE as isize {
                consecutive = false;
            }
        }

        for &p in &std_ptrs {
            thread_free::<StandardPolicy, Backend>(p);
        }
        consecutive
    })
    .join()
    .expect("standard-policy allocation worker thread panicked");

    // 2. Run SecurePolicy check in a separate thread
    let sec_consecutive = std::thread::spawn(move || unsafe {
        let mut sec_ptrs = [core::ptr::null_mut::<u8>(); 5];
        for slot in &mut sec_ptrs {
            *slot = thread_alloc::<SecurePolicy, Backend>(SIZE, ALIGN);
            assert!(!slot.is_null());
        }

        let mut consecutive = true;
        for i in 0..4 {
            let diff = (sec_ptrs[i + 1] as isize - sec_ptrs[i] as isize).abs();
            if diff != SIZE as isize {
                consecutive = false;
            }
        }

        for &p in &sec_ptrs {
            thread_free::<SecurePolicy, Backend>(p);
        }
        consecutive
    })
    .join()
    .expect("secure-policy allocation worker thread panicked");

    assert!(
        std_consecutive,
        "StandardPolicy allocations must be consecutive"
    );
    assert!(
        !sec_consecutive,
        "SecurePolicy allocations must be non-consecutive (randomized)"
    );
}

#[test]
fn test_in_place_realloc_growth_under_policies() {
    use core::alloc::Layout;
    use mnemosyne_core::policy::{private::Sealed, AllocPolicy};

    const ALIGN: usize = 8;
    // 20 bytes rounded to size class 1 (32 bytes).
    let old_layout = Layout::from_size_align(20, ALIGN)
        .expect("20-byte allocation with 8-byte alignment is a valid Layout");
    // 30 bytes still maps to size class 1 (32 bytes).
    let new_size = 30;

    unsafe {
        // 1. Custom ZeroInitOnlyPolicy: check that in-place growth works and zeroes the new range
        #[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
        struct ZeroInitOnlyPolicy;
        impl Sealed for ZeroInitOnlyPolicy {}
        impl AllocPolicy for ZeroInitOnlyPolicy {
            const ENABLE_POISONING: bool = false;
            const ZERO_INITIALIZE: bool = true;
        }

        let ptr1 = thread_alloc::<ZeroInitOnlyPolicy, Backend>(20, ALIGN);
        assert!(!ptr1.is_null());
        core::ptr::write_bytes(ptr1, 0xCC, 20);

        let ptr1_re = thread_realloc::<ZeroInitOnlyPolicy, Backend>(ptr1, old_layout, new_size);
        assert!(!ptr1_re.is_null());
        assert_eq!(
            ptr1_re, ptr1,
            "ZeroInitOnlyPolicy reallocation must be in-place"
        );

        // Check content preservation
        for i in 0..20 {
            assert_eq!(*ptr1_re.add(i), 0xCC);
        }
        // Check new range zeroing
        for i in 20..30 {
            assert_eq!(*ptr1_re.add(i), 0);
        }
        thread_free::<ZeroInitOnlyPolicy, Backend>(ptr1_re);

        // 2. Custom PoisonOnlyPolicy: check in-place growth and poisoning
        #[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
        struct PoisonOnlyPolicy;
        impl Sealed for PoisonOnlyPolicy {}
        impl AllocPolicy for PoisonOnlyPolicy {
            const ENABLE_POISONING: bool = true;
            const ZERO_INITIALIZE: bool = false;
        }

        let ptr2 = thread_alloc::<PoisonOnlyPolicy, Backend>(20, ALIGN);
        assert!(!ptr2.is_null());
        core::ptr::write_bytes(ptr2, 0xDD, 20);

        let ptr2_re = thread_realloc::<PoisonOnlyPolicy, Backend>(ptr2, old_layout, new_size);
        assert!(!ptr2_re.is_null());
        assert_eq!(
            ptr2_re, ptr2,
            "PoisonOnlyPolicy reallocation must be in-place"
        );

        // Check content preservation
        for i in 0..20 {
            assert_eq!(*ptr2_re.add(i), 0xDD);
        }
        // Check new range initialization to POISON_ALLOC_BYTE (0xAD)
        for i in 20..30 {
            assert_eq!(*ptr2_re.add(i), 0xAD);
        }
        thread_free::<PoisonOnlyPolicy, Backend>(ptr2_re);

        // 3. SecurePolicy: check in-place growth and zeroing + poisoning
        let ptr3 = thread_alloc::<SecurePolicy, Backend>(20, ALIGN);
        assert!(!ptr3.is_null());
        core::ptr::write_bytes(ptr3, 0xEE, 20);

        let ptr3_re = thread_realloc::<SecurePolicy, Backend>(ptr3, old_layout, new_size);
        assert!(!ptr3_re.is_null());
        assert_eq!(ptr3_re, ptr3, "SecurePolicy reallocation must be in-place");

        // Check content preservation
        for i in 0..20 {
            assert_eq!(*ptr3_re.add(i), 0xEE);
        }
        // Check new range zero-initialization
        for i in 20..30 {
            assert_eq!(*ptr3_re.add(i), 0);
        }
        thread_free::<SecurePolicy, Backend>(ptr3_re);

        // 4. HardenedPolicy: check in-place growth and zeroing + poisoning
        let ptr4 = thread_alloc::<HardenedPolicy, Backend>(20, ALIGN);
        assert!(!ptr4.is_null());
        core::ptr::write_bytes(ptr4, 0xFF, 20);

        let ptr4_re = thread_realloc::<HardenedPolicy, Backend>(ptr4, old_layout, new_size);
        assert!(!ptr4_re.is_null());
        assert_eq!(
            ptr4_re, ptr4,
            "HardenedPolicy reallocation must be in-place"
        );

        // Check content preservation
        for i in 0..20 {
            assert_eq!(*ptr4_re.add(i), 0xFF);
        }
        // Check new range zero-initialization
        for i in 20..30 {
            assert_eq!(*ptr4_re.add(i), 0);
        }
        thread_free::<HardenedPolicy, Backend>(ptr4_re);
    }
}
