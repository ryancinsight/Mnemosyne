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
use mnemosyne_core::StandardPolicy;
use mnemosyne_core::policy::{HardenedPolicy, SecurePolicy};
use mnemosyne_local::{thread_alloc, thread_free, thread_realloc, usable_size};

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
    }
}

/// `HardenedPolicy` realloc byte preservation + expanded zeroing, in its own
/// process so the encrypted-policy TLS slot owns the segment (ADR 0001).
#[test]
fn test_realloc_under_hardened_policy() {
    use core::alloc::Layout;

    const ALIGN: usize = 8;
    let old_layout = Layout::from_size_align(16, ALIGN)
        .expect("16-byte allocation with 8-byte alignment is a valid Layout");
    let new_size = 64;

    unsafe {
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

        assert!(usable_size(ptr_std) >= SIZE);
        assert!(usable_size(ptr_sec) >= SIZE);

        thread_free::<StandardPolicy, Backend>(ptr_std);
        thread_free::<SecurePolicy, Backend>(ptr_sec);
    }
}

/// `usable_size` accuracy under the encrypted policy, in its own process (one
/// encryption mode per backend; ADR 0001).
#[test]
fn test_usable_size_accuracy_under_hardened_policy() {
    const SIZE: usize = 48;
    const ALIGN: usize = 8;
    unsafe {
        let ptr_hrd = thread_alloc::<HardenedPolicy, Backend>(SIZE, ALIGN);
        assert!(usable_size(ptr_hrd) >= SIZE);
        thread_free::<HardenedPolicy, Backend>(ptr_hrd);
    }
}

#[test]
fn test_secure_and_standard_policies_preserve_hardened_segment_encoding() {
    use core::alloc::Layout;

    const SIZE: usize = 32;
    const ALIGN: usize = 8;
    const ROUNDS: usize = 8;
    unsafe {
        let hardened_first = thread_alloc::<HardenedPolicy, Backend>(SIZE, ALIGN);
        let hardened_second = thread_alloc::<HardenedPolicy, Backend>(SIZE, ALIGN);
        let hardened_third = thread_alloc::<HardenedPolicy, Backend>(SIZE, ALIGN);
        let hardened_fourth = thread_alloc::<HardenedPolicy, Backend>(SIZE, ALIGN);
        assert!(!hardened_first.is_null());
        assert!(!hardened_second.is_null());
        assert!(!hardened_third.is_null());
        assert!(!hardened_fourth.is_null());

        // Two unencrypted policies free hardened blocks. Each must consult the
        // owning segment's recorded mode and encode the link with the hardened
        // cookie; getting that wrong corrupts the chain for every later
        // hardened allocation. The old form searched for the freed address
        // itself, which the randomized free-list head makes a coin flip
        // (ADR 0001, revised 2026-09-09); the chain's integrity is what is
        // actually claimed, so that is what is asserted.
        thread_free::<StandardPolicy, Backend>(hardened_second);
        thread_free::<SecurePolicy, Backend>(hardened_fourth);

        let mut blocks = [core::ptr::null_mut(); ROUNDS];
        for (round, block) in blocks.iter_mut().enumerate() {
            *block = claim_and_stamp::<HardenedPolicy>(SIZE, ALIGN, round as u8);
        }
        for (round, block) in blocks.iter().enumerate() {
            verify_stamp(*block, SIZE, round as u8, "hardened");
            assert_ne!(
                *block, hardened_first,
                "the chain handed back a block that is still live"
            );
            assert_ne!(
                *block, hardened_third,
                "the chain handed back a block that is still live"
            );
            for earlier in &blocks[..round] {
                assert_ne!(
                    block, earlier,
                    "the hardened chain handed out one block twice"
                );
            }
        }
        for block in blocks {
            thread_free::<HardenedPolicy, Backend>(block);
        }

        // A grow across size classes may relocate, so the contract is that the
        // owner's payload survives the move -- not that the address is
        // unchanged. The old form asserted the address and passed only because
        // the preceding probe loop happened to leave a larger block adjacent.
        core::ptr::write_bytes(hardened_first, 0xA5, SIZE);
        let layout = Layout::from_size_align(SIZE, ALIGN)
            .expect("invariant: SIZE is a multiple of the power-of-two ALIGN");
        let resized = thread_realloc::<SecurePolicy, Backend>(hardened_first, layout, 64);
        assert!(!resized.is_null());
        assert!(
            resized.addr().is_multiple_of(ALIGN),
            "resized block is misaligned"
        );
        verify_stamp(resized, SIZE, 0xA5, "secure-realloc");

        thread_free::<HardenedPolicy, Backend>(hardened_third);
        thread_free::<SecurePolicy, Backend>(resized);
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
    use mnemosyne_core::policy::{AllocPolicy, private::Sealed};

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
        impl_unencrypted_policy_slot!(ZeroInitOnlyPolicy);
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
        impl_unencrypted_policy_slot!(PoisonOnlyPolicy);
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
    }
}

/// `HardenedPolicy` in-place realloc growth + zeroing, in its own process (one
/// encryption mode per backend; ADR 0001).
#[test]
fn test_in_place_realloc_growth_under_hardened_policy() {
    use core::alloc::Layout;

    const ALIGN: usize = 8;
    let old_layout = Layout::from_size_align(20, ALIGN)
        .expect("20-byte allocation with 8-byte alignment is a valid Layout");
    let new_size = 30;

    unsafe {
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

/// Pins ADR 0001's mode-keyed routing: a hardened-policy free of a standard
/// allocation must use the owning segment's unencrypted encoding, then the
/// standard allocator must be able to reuse that exact block.
#[test]
fn mixed_encryption_modes_round_trip_without_corruption() {
    const SIZE: usize = 48;
    const ALIGN: usize = 8;
    const ROUNDS: usize = 8;
    unsafe {
        let ptr_std = thread_alloc::<StandardPolicy, Backend>(SIZE, ALIGN);
        assert!(!ptr_std.is_null());
        let ptr_hrd = thread_alloc::<HardenedPolicy, Backend>(SIZE, ALIGN);
        assert!(!ptr_hrd.is_null());

        // The freeing policy intentionally differs from the allocation policy.
        thread_free::<HardenedPolicy, Backend>(ptr_hrd);
        thread_free::<HardenedPolicy, Backend>(ptr_std);

        // A mis-decoded link hands back a wild pointer, so reuse is checked by
        // what the blocks do rather than by which address comes back: every
        // block is distinct, aligned, and round-trips a payload written
        // through it. Pointer identity is not the property (ADR 0001,
        // revised 2026-09-09) -- the free lists randomize their head.
        let mut std_blocks = [core::ptr::null_mut(); ROUNDS];
        let mut hrd_blocks = [core::ptr::null_mut(); ROUNDS];
        for (round, (std_block, hrd_block)) in
            std_blocks.iter_mut().zip(hrd_blocks.iter_mut()).enumerate()
        {
            *std_block = claim_and_stamp::<StandardPolicy>(SIZE, ALIGN, round as u8);
            *hrd_block = claim_and_stamp::<HardenedPolicy>(SIZE, ALIGN, !(round as u8));
        }
        for (round, (std_block, hrd_block)) in std_blocks.iter().zip(hrd_blocks.iter()).enumerate()
        {
            verify_stamp(*std_block, SIZE, round as u8, "standard");
            verify_stamp(*hrd_block, SIZE, !(round as u8), "hardened");
            for earlier in &std_blocks[..round] {
                assert_ne!(
                    std_block, earlier,
                    "the standard free list handed out one block twice"
                );
            }
            for earlier in &hrd_blocks[..round] {
                assert_ne!(
                    hrd_block, earlier,
                    "the hardened free list handed out one block twice"
                );
            }
            assert_ne!(
                std_block, hrd_block,
                "the two encoding modes must not share a block"
            );
        }
        for (std_block, hrd_block) in std_blocks.into_iter().zip(hrd_blocks) {
            thread_free::<StandardPolicy, Backend>(std_block);
            thread_free::<HardenedPolicy, Backend>(hrd_block);
        }
    }
}

/// Allocate one block under `P` and write `mark` across its whole payload.
///
/// # Safety
/// The caller owns the returned block until it frees it under a policy whose
/// segment mode matches, per ADR 0001.
unsafe fn claim_and_stamp<P>(size: usize, align: usize, mark: u8) -> *mut u8
where
    P: mnemosyne_local::tls_slot::PolicySlotSelection<Backend> + mnemosyne_core::AllocPolicy,
{
    unsafe {
        let block = thread_alloc::<P, Backend>(size, align);
        assert!(!block.is_null(), "the free list must satisfy a reuse");
        assert!(block.addr().is_multiple_of(align), "block is misaligned");
        core::ptr::write_bytes(block, mark, size);
        block
    }
}

/// Every byte of the block still reads back the mark written into it.
///
/// # Safety
/// `block` is a live allocation of at least `size` bytes.
unsafe fn verify_stamp(block: *mut u8, size: usize, mark: u8, mode: &str) {
    unsafe {
        let payload = core::slice::from_raw_parts(block, size);
        for (offset, byte) in payload.iter().enumerate() {
            assert_eq!(
                *byte, mark,
                "{mode} block corrupted at offset {offset}: a mis-decoded link overlapped it"
            );
        }
    }
}
