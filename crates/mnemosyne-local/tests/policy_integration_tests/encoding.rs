//! Free-list encoding across a policy boundary (ADR 0001): a chain
//! encoded under one mode stays decodable when another frees into it.

use mnemosyne_backend::MemoryBackendWrapper as Backend;
use mnemosyne_core::StandardPolicy;
use mnemosyne_core::policy::{HardenedPolicy, SecurePolicy};
use mnemosyne_local::{thread_alloc, thread_free, thread_realloc};

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
