//! Unit tests for scratch buffer and pools.

extern crate std;
use super::aligned_vec::AlignedVec;
use super::bank::ScratchBank;
use super::element::DEFAULT_SCRATCH_ALIGN;
use super::pool::{MAX_POOL_SLOTS, ScratchPool};

#[test]
fn aligned_vec_capacity_and_alignment() {
    let mut v = AlignedVec::<f64>::with_capacity(256);
    v.ensure_len(256);
    assert_eq!(v.len(), 256);
    assert!(v.capacity() >= 256);
    assert_eq!(v.as_mut_ptr() as usize % DEFAULT_SCRATCH_ALIGN, 0);
}

#[test]
fn aligned_vec_growth_preserves_data() {
    let mut v = AlignedVec::<f32>::with_capacity(4);
    v.ensure_len(4);
    v.as_mut_slice().copy_from_slice(&[1.0, 2.0, 3.0, 4.0]);
    v.ensure_len(8);
    assert_eq!(&v.as_slice()[..4], &[1.0, 2.0, 3.0, 4.0]);
    assert_eq!(v.len(), 8);
}

#[test]
fn aligned_vec_zero_capacity_is_valid() {
    let v = AlignedVec::<f32>::dangling();
    assert_eq!(v.len(), 0);
    assert!(v.is_empty());
    assert_eq!(v.capacity(), 0);
}

#[test]
fn aligned_vec_into_vec() {
    let mut v = AlignedVec::<f64>::with_capacity(4);
    v.ensure_len(4);
    let expected = [1.0, 2.0, 3.0, 4.0];
    v.as_mut_slice().copy_from_slice(&expected);
    let vec = v.into_vec();
    assert_eq!(vec.as_slice(), expected);
    assert_eq!(vec.len(), expected.len());
}

#[test]
fn scratch_pool_single_borrow() {
    let pool = ScratchPool::<f64>::new();
    pool.with_scratch(128, |scratch| {
        assert_eq!(scratch.len(), 128, "must return exactly n elements");
        scratch[0] = 42.0;
        assert_eq!(scratch[0], 42.0);
        assert_eq!(scratch.as_ptr() as usize % DEFAULT_SCRATCH_ALIGN, 0);
    });
    assert_eq!(pool.borrow_depth(), 0);
}

#[test]
fn scratch_pool_nested_borrows() {
    let pool = ScratchPool::<f32>::new();
    pool.with_scratch(64, |s1| {
        s1[0] = 1.0;
        assert_eq!(pool.borrow_depth(), 1);
        pool.with_scratch(128, |s2| {
            s2[0] = 2.0;
            assert_eq!(pool.borrow_depth(), 2);
            assert_eq!(s1[0], 1.0);
            assert_eq!(s2[0], 2.0);
        });
        assert_eq!(pool.borrow_depth(), 1);
    });
    assert_eq!(pool.borrow_depth(), 0);
}

#[test]
fn scratch_pool_overflow_to_owned() {
    let pool = ScratchPool::<f64>::new();
    fn nest(pool: &ScratchPool<f64>, depth: usize) {
        if depth == 0 {
            return;
        }
        pool.with_scratch(32, |_| {
            nest(pool, depth - 1);
        });
    }
    nest(&pool, MAX_POOL_SLOTS + 1);
    assert_eq!(pool.borrow_depth(), 0);
}

#[test]
fn scratch_pool_exact_length() {
    let pool = ScratchPool::<f64>::new();
    // First call: grow to 256.
    pool.with_scratch(256, |s| assert_eq!(s.len(), 256));
    // Second call: request 128 — must get exactly 128, not 256.
    pool.with_scratch(128, |s| assert_eq!(s.len(), 128));
    // Third call: request 512 — grows.
    pool.with_scratch(512, |s| assert_eq!(s.len(), 512));
}

#[test]
fn scratch_pool_no_rezero_on_reuse() {
    let pool = ScratchPool::<f64>::new();
    // Write data.
    pool.with_scratch(64, |s| {
        for (i, v) in s.iter_mut().enumerate() {
            *v = i as f64;
        }
    });
    // Reuse — data should still be present (not re-zeroed).
    pool.with_scratch(64, |s| {
        assert_eq!(s[0], 0.0); // first element was 0.0
        assert_eq!(s[63], 63.0); // last element was 63.0
    });
}

#[test]
fn scratch_pool_returns_value() {
    let pool = ScratchPool::<f64>::new();
    let sum = pool.with_scratch(100, |scratch| {
        for (i, v) in scratch.iter_mut().enumerate() {
            *v = i as f64;
        }
        scratch.iter().sum::<f64>()
    });
    assert_eq!(sum, (0..100).map(|i| i as f64).sum::<f64>());
}

#[cfg(feature = "eunomia")]
#[test]
fn eunomia_scratch_pool_preserves_values() {
    let single = ScratchPool::<eunomia::Complex<f32>>::new();
    single.with_scratch(2, |scratch| {
        assert_eq!(scratch.len(), 2);
        assert_eq!(scratch[0], eunomia::Complex::new(0.0, 0.0));
        scratch[0] = eunomia::Complex::new(1.25, -2.5);
        scratch[1] = eunomia::Complex::new(3.5, 4.75);
    });
    single.with_scratch(2, |scratch| {
        assert_eq!(scratch[0], eunomia::Complex::new(1.25, -2.5));
        assert_eq!(scratch[1], eunomia::Complex::new(3.5, 4.75));
    });

    let double = ScratchPool::<eunomia::Complex<f64>>::new();
    double.with_scratch(1, |scratch| {
        assert_eq!(scratch.as_ptr() as usize % DEFAULT_SCRATCH_ALIGN, 0);
        assert_eq!(scratch[0], eunomia::Complex::new(0.0, 0.0));
        scratch[0] = eunomia::Complex::new(-8.0, 13.0);
    });
    double.with_scratch(1, |scratch| {
        assert_eq!(scratch[0], eunomia::Complex::new(-8.0, 13.0));
    });
}

#[test]
fn with_slot_capacity_preallocates() {
    let pool = ScratchPool::<f32>::with_slot_capacity(512);
    pool.with_scratch(256, |scratch| {
        assert_eq!(scratch.len(), 256);
        assert_eq!(scratch.as_ptr() as usize % DEFAULT_SCRATCH_ALIGN, 0);
    });
}

#[test]
fn scratch_bank_slots_are_independent() {
    let bank = ScratchBank::<f64, 2>::new();
    bank.with_scratch::<0, _>(128, |first| {
        first[0] = 11.0;
        bank.with_scratch::<1, _>(64, |second| {
            second[0] = 29.0;
            assert_eq!(first[0], 11.0);
            assert_eq!(second[0], 29.0);
            assert_eq!(second.len(), 64);
        });
        assert_eq!(first[0], 11.0);
        assert_eq!(first.len(), 128);
    });
    assert!(bank.capacity::<0>() >= 128);
    assert!(bank.capacity::<1>() >= 64);
    assert_eq!(bank.borrow_depth::<0>(), 0);
    assert_eq!(bank.borrow_depth::<1>(), 0);
}

/// `capacity()` is reachable from inside a live `with_scratch` borrow through
/// entirely safe code, because both take `&self` and the pool is shared (its
/// documented home is `thread_local!`). Slot 0 backs both the depth-0 borrow and
/// the accessor, so the accessor must not derive a reference into the slot the
/// borrow already holds exclusively.
#[test]
fn capacity_is_readable_inside_a_live_borrow() {
    let pool = ScratchPool::<f64>::new();
    let capacity = pool.with_scratch(128, |_| pool.capacity());
    assert_eq!(capacity, 128, "slot 0 grew to exactly the requested length");
}

/// The same reentrancy, with the scratch slice used *after* the accessor call.
/// This is the shape a real caller has (read the capacity, then keep writing),
/// and the one that makes an aliasing violation observable: a shared read that
/// invalidated the live `&mut` would be caught here on the subsequent write.
#[test]
fn scratch_stays_writable_after_a_reentrant_capacity_read() {
    let pool = ScratchPool::<f64>::new();
    let (capacity, written) = pool.with_scratch(64, |scratch| {
        let capacity = pool.capacity();
        scratch[0] = 7.5;
        scratch[63] = -1.25;
        (capacity, scratch[0] + scratch[63])
    });
    assert_eq!(capacity, 64);
    assert_eq!(written, 6.25);
}

/// Nested borrows report slot 0's capacity, not the inner slot's: the inner
/// `with_scratch` takes slot 1, so growing it must not move the primary figure.
#[test]
fn capacity_tracks_the_primary_slot_across_nested_borrows() {
    let pool = ScratchPool::<f32>::new();
    pool.with_scratch(32, |outer| {
        outer[0] = 1.0;
        let outer_capacity = pool.capacity();
        assert_eq!(outer_capacity, 32);
        pool.with_scratch(256, |inner| {
            inner[0] = 2.0;
            assert_eq!(
                pool.capacity(),
                32,
                "the inner borrow uses slot 1, so slot 0's capacity is unchanged"
            );
        });
        assert_eq!(outer[0], 1.0);
    });
    assert_eq!(pool.capacity(), 32);
}

/// The bank accessor forwards to the pool accessor, so it inherits the same
/// reentrancy: slot `INDEX`'s capacity must be readable from inside that slot's
/// own live borrow.
#[test]
fn bank_capacity_is_readable_inside_a_live_borrow() {
    let bank = ScratchBank::<f64, 2>::new();
    let capacity = bank.with_scratch::<1, _>(48, |scratch| {
        let capacity = bank.capacity::<1>();
        scratch[0] = 3.0;
        capacity
    });
    assert_eq!(capacity, 48);
    assert_eq!(bank.capacity::<0>(), 0, "slot 0 was never borrowed");
}

#[test]
fn test_scratch_pool_panic_resilience() {
    let pool = ScratchPool::<f64>::new();
    let pool_ref = std::panic::AssertUnwindSafe(&pool);
    let result = std::panic::catch_unwind(move || {
        pool_ref.with_scratch(128, |_scratch| {
            assert_eq!(pool_ref.borrow_depth(), 1);
            panic!("intended panic inside closure");
        });
    });
    assert!(result.is_err());
    assert_eq!(
        pool.borrow_depth(),
        0,
        "borrow depth must be restored to 0 after panic!"
    );
}

#[test]
fn aligned_vec_push_grows_and_preserves_order() {
    let mut v = AlignedVec::<f32>::with_capacity(2);
    for i in 0..9 {
        v.push(i as f32);
    }
    assert_eq!(v.len(), 9);
    assert!(v.capacity() >= 9);
    assert_eq!(v.as_slice(), &[0.0, 1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0]);
    assert_eq!(v.as_mut_ptr() as usize % DEFAULT_SCRATCH_ALIGN, 0);
}

#[test]
fn aligned_vec_extend_from_slice_appends_after_existing() {
    let mut v = AlignedVec::<u32>::from_slice(&[1, 2, 3]);
    v.extend_from_slice(&[4, 5]);
    v.extend_from_slice(&[]);
    assert_eq!(v.as_slice(), &[1, 2, 3, 4, 5]);
}

#[test]
fn aligned_vec_from_slice_copies_the_source() {
    let source = [1.5_f64, 2.5, 3.5];
    let mut v = AlignedVec::from_slice(&source);
    v.as_mut_slice()[0] = 9.0;
    assert_eq!(source[0], 1.5);
    assert_eq!(v.as_slice(), &[9.0, 2.5, 3.5]);
}

#[test]
fn aligned_vec_filled_writes_every_element() {
    let v = AlignedVec::filled(5, 7_u8);
    assert_eq!(v.as_slice(), &[7, 7, 7, 7, 7]);
    assert_eq!(AlignedVec::filled(0, 7_u8).len(), 0);
}

#[test]
fn aligned_vec_resize_grows_with_value_and_shrinks_in_place() {
    let mut v = AlignedVec::<i32>::from_slice(&[1, 2]);
    v.resize(5, -1);
    assert_eq!(v.as_slice(), &[1, 2, -1, -1, -1]);
    let grown = v.capacity();
    v.resize(2, 0);
    assert_eq!(v.as_slice(), &[1, 2]);
    assert_eq!(v.capacity(), grown, "shrinking keeps the allocation");
    v.resize(4, 8);
    assert_eq!(v.as_slice(), &[1, 2, 8, 8]);
}

#[test]
fn aligned_vec_is_send_and_sync() {
    fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<AlignedVec<f64>>();
}
