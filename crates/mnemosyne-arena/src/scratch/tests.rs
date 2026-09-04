//! Unit tests for scratch buffer and pools.

extern crate std;
use super::aligned_vec::AlignedVec;
use super::bank::ScratchBank;
use super::element::DEFAULT_SCRATCH_ALIGN;
use super::pool::{ScratchPool, MAX_POOL_SLOTS};

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

// ── Phase 11: truncate / retain / iterators / collection ─────────────────────

#[test]
fn aligned_vec_truncate_shortens_and_noop_when_larger() {
    let mut v = AlignedVec::<i32>::from_slice(&[1, 2, 3, 4, 5]);
    v.truncate(3);
    assert_eq!(v.as_slice(), &[1, 2, 3]);
    let cap = v.capacity();
    v.truncate(10); // no-op
    assert_eq!(v.len(), 3);
    assert_eq!(v.capacity(), cap, "allocation not shrunk");
}

#[test]
fn aligned_vec_retain_keeps_matching_elements() {
    let mut v = AlignedVec::<i32>::from_slice(&[1, 2, 3, 4, 5, 6]);
    v.retain(|&x| x % 2 == 0);
    assert_eq!(v.as_slice(), &[2, 4, 6]);
}

#[test]
fn aligned_vec_retain_all_or_none() {
    let mut v = AlignedVec::<u8>::from_slice(&[10, 20, 30]);
    v.retain(|_| true);
    assert_eq!(v.as_slice(), &[10, 20, 30]);
    v.retain(|_| false);
    assert_eq!(v.len(), 0);
    assert!(v.is_empty());
}

#[test]
fn aligned_vec_borrowed_into_iter() {
    let v = AlignedVec::<f32>::from_slice(&[1.0, 2.0, 3.0]);
    let sum: f32 = v.iter().sum();
    assert_eq!(sum, 6.0);
    // also via IntoIterator for &AlignedVec
    let sum2: f32 = (&v).into_iter().copied().sum();
    assert_eq!(sum2, 6.0);
}

#[test]
fn aligned_vec_mut_borrowed_into_iter() {
    let mut v = AlignedVec::<i32>::from_slice(&[1, 2, 3]);
    for x in &mut v {
        *x *= 2;
    }
    assert_eq!(v.as_slice(), &[2, 4, 6]);
}

#[test]
fn aligned_vec_owned_into_iter() {
    let v = AlignedVec::<u32>::from_slice(&[10, 20, 30]);
    let collected: std::vec::Vec<u32> = v.into_iter().collect();
    assert_eq!(collected, &[10, 20, 30]);
}

#[test]
fn aligned_vec_owned_into_iter_exact_size() {
    let v = AlignedVec::<u8>::from_slice(&[1, 2, 3, 4]);
    let mut it = v.into_iter();
    assert_eq!(it.len(), 4);
    let _ = it.next();
    assert_eq!(it.len(), 3);
}

#[test]
fn aligned_vec_from_iterator() {
    let v: AlignedVec<u32> = (0u32..5).collect();
    assert_eq!(v.as_slice(), &[0, 1, 2, 3, 4]);
}

#[test]
fn aligned_vec_extend_from_iterator() {
    let mut v = AlignedVec::<u32>::from_slice(&[0, 1, 2]);
    v.extend(3u32..6);
    assert_eq!(v.as_slice(), &[0, 1, 2, 3, 4, 5]);
}

#[test]
fn aligned_vec_extend_from_ref_iterator() {
    let extra = [10u8, 20, 30];
    let mut v = AlignedVec::<u8>::from_slice(&[1, 2]);
    v.extend(extra.iter());
    assert_eq!(v.as_slice(), &[1, 2, 10, 20, 30]);
}

#[test]
fn aligned_vec_bool_is_scratch_element() {
    let mut v = AlignedVec::<bool>::zeroed(4);
    assert_eq!(v.as_slice(), &[false, false, false, false]);
    v[1] = true;
    v[3] = true;
    assert_eq!(v.as_slice(), &[false, true, false, true]);
    v.fill(false);
    assert!(v.iter().all(|&b| !b));
    v.clear();
    assert!(v.is_empty());
}

// ── Phase 13: shrink_to_fit / swap_remove / append / split_off / spare_capacity_mut ──

#[test]
fn aligned_vec_shrink_to_fit_releases_excess() {
    let mut v = AlignedVec::<u32>::with_capacity(256);
    for i in 0..4u32 { v.push(i); }
    assert!(v.capacity() >= 256);
    v.shrink_to_fit();
    assert_eq!(v.len(), 4);
    assert!(v.capacity() < 256, "capacity should have shrunk");
    assert_eq!(v.as_slice(), &[0, 1, 2, 3]);
}

#[test]
fn aligned_vec_shrink_to_respects_min_capacity() {
    let mut v = AlignedVec::<u32>::with_capacity(256);
    v.push(1);
    v.shrink_to(64);
    assert_eq!(v.len(), 1);
    assert!(v.capacity() >= 64, "capacity must be at least min_capacity");
    assert!(v.capacity() < 256, "capacity should have shrunk");
}

#[test]
fn aligned_vec_shrink_to_fit_empty_frees_alloc() {
    let mut v = AlignedVec::<u64>::with_capacity(128);
    v.shrink_to_fit();
    assert_eq!(v.capacity(), 0);
    assert_eq!(v.len(), 0);
}

#[test]
fn aligned_vec_swap_remove_preserves_others() {
    let mut v = AlignedVec::<i32>::from_slice(&[10, 20, 30, 40, 50]);
    let removed = v.swap_remove(1); // removes 20, swaps with 50
    assert_eq!(removed, 20);
    assert_eq!(v.len(), 4);
    // Last element (50) fills the gap
    assert!(v.as_slice().contains(&10));
    assert!(v.as_slice().contains(&30));
    assert!(v.as_slice().contains(&40));
    assert!(v.as_slice().contains(&50));
    assert!(!v.as_slice().contains(&20));
}

#[test]
fn aligned_vec_swap_remove_last() {
    let mut v = AlignedVec::<u8>::from_slice(&[1, 2, 3]);
    let removed = v.swap_remove(2);
    assert_eq!(removed, 3);
    assert_eq!(v.as_slice(), &[1, 2]);
}

#[test]
fn aligned_vec_append_drains_source() {
    let mut a = AlignedVec::<u32>::from_slice(&[1, 2, 3]);
    let mut b = AlignedVec::<u32>::from_slice(&[4, 5, 6]);
    a.append(&mut b);
    assert_eq!(a.as_slice(), &[1, 2, 3, 4, 5, 6]);
    assert!(b.is_empty(), "source must be empty after append");
}

#[test]
fn aligned_vec_split_off_at_midpoint() {
    let mut v = AlignedVec::<i32>::from_slice(&[1, 2, 3, 4, 5]);
    let tail = v.split_off(2);
    assert_eq!(v.as_slice(), &[1, 2]);
    assert_eq!(tail.as_slice(), &[3, 4, 5]);
}

#[test]
fn aligned_vec_split_off_at_zero_is_clone_and_clear() {
    let mut v = AlignedVec::<u8>::from_slice(&[10, 20, 30]);
    let tail = v.split_off(0);
    assert!(v.is_empty());
    assert_eq!(tail.as_slice(), &[10, 20, 30]);
}

#[test]
fn aligned_vec_spare_capacity_mut_write_then_set_len() {
    let mut v = AlignedVec::<u32>::with_capacity(8);
    v.push(0);
    // Write into spare capacity
    let spare = v.spare_capacity_mut();
    // SAFETY: spare covers [len, capacity) = [1, 8); we write exactly 3 elements
    unsafe {
        let spare_slice = &mut *spare;
        spare_slice[0] = 1;
        spare_slice[1] = 2;
        spare_slice[2] = 3;
        v.set_len_unchecked(4);
    }
    assert_eq!(v.as_slice(), &[0, 1, 2, 3]);
}

#[test]
fn aligned_vec_must_use_zeroed_compiles_with_discard_allowed() {
    // Compile-time test: calling zeroed() and immediately dropping is a
    // user choice; #[must_use] warns but does not error.
    let _ = AlignedVec::<u32>::zeroed(4);
}

#[test]
fn aligned_vec_drain_yields_range_and_collapses() {
    let mut v = AlignedVec::<i32>::from_slice(&[10, 20, 30, 40, 50]);
    let drained: std::vec::Vec<i32> = v.drain(1, 3).collect();
    assert_eq!(drained, &[20, 30]);
    assert_eq!(v.as_slice(), &[10, 40, 50]);
}

#[test]
fn aligned_vec_drain_full_range() {
    let mut v = AlignedVec::<u8>::from_slice(&[1, 2, 3]);
    let d: std::vec::Vec<u8> = v.drain(0, 3).collect();
    assert_eq!(d, &[1, 2, 3]);
    assert!(v.is_empty());
}

#[test]
fn aligned_vec_drain_empty_range_is_noop() {
    let mut v = AlignedVec::<u32>::from_slice(&[5, 6, 7]);
    let d: std::vec::Vec<u32> = v.drain(1, 1).collect();
    assert!(d.is_empty());
    assert_eq!(v.as_slice(), &[5, 6, 7]);
}

#[test]
fn aligned_vec_drain_drop_without_consume() {
    let mut v = AlignedVec::<i32>::from_slice(&[1, 2, 3, 4, 5]);
    {
        let _drain = v.drain(1, 4); // drop without consuming
    }
    assert_eq!(v.as_slice(), &[1, 5]);
}

#[test]
fn aligned_vec_into_iter_rev() {
    let v = AlignedVec::<i32>::from_slice(&[1, 2, 3, 4]);
    let reversed: std::vec::Vec<i32> = v.into_iter().rev().collect();
    assert_eq!(reversed, &[4, 3, 2, 1]);
}

#[test]
fn aligned_vec_into_iter_front_and_back() {
    let v = AlignedVec::<u32>::from_slice(&[1, 2, 3, 4, 5]);
    let mut it = v.into_iter();
    assert_eq!(it.next(), Some(1));
    assert_eq!(it.next_back(), Some(5));
    assert_eq!(it.next(), Some(2));
    assert_eq!(it.next_back(), Some(4));
    assert_eq!(it.next(), Some(3));
    assert_eq!(it.next(), None);
    assert_eq!(it.next_back(), None);
}
