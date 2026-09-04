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
fn aligned_vec_clear_resets_len_without_reallocating() {
    let mut v = AlignedVec::<f32>::zeroed(16);
    let ptr_before = v.as_mut_ptr();
    let cap_before = v.capacity();
    v.clear();
    assert_eq!(v.len(), 0);
    assert!(v.is_empty());
    assert_eq!(v.capacity(), cap_before);
    assert_eq!(v.as_mut_ptr(), ptr_before);
}

#[test]
fn aligned_vec_push_appends_elements() {
    let mut v = AlignedVec::<f64>::with_capacity(4);
    for i in 0..4_u64 {
        v.push(i as f64);
    }
    assert_eq!(v.len(), 4);
    assert_eq!(&v[..], &[0.0, 1.0, 2.0, 3.0]);
}

#[test]
fn aligned_vec_push_grows_when_capacity_exceeded() {
    let mut v = AlignedVec::<f32>::with_capacity(2);
    v.push(1.0);
    v.push(2.0);
    v.push(3.0); // triggers growth
    assert_eq!(v.len(), 3);
    assert!(v.capacity() >= 3);
    assert_eq!(&v[..], &[1.0_f32, 2.0, 3.0]);
}

#[test]
fn aligned_vec_extend_from_slice_appends_all() {
    let mut v = AlignedVec::<f64>::with_capacity(4);
    v.extend_from_slice(&[1.0, 2.0, 3.0]);
    v.extend_from_slice(&[4.0, 5.0]);
    assert_eq!(v.len(), 5);
    assert_eq!(&v[..], &[1.0, 2.0, 3.0, 4.0, 5.0]);
}

#[test]
fn aligned_vec_clear_then_extend_reuses_allocation() {
    let mut v = AlignedVec::<f64>::with_capacity(8);
    v.extend_from_slice(&[10.0, 20.0, 30.0]);
    let cap_after_first_fill = v.capacity();
    let ptr_after_first_fill = v.as_mut_ptr();
    v.clear();
    v.extend_from_slice(&[1.0, 2.0]);
    // Same allocation reused — no realloc when length is smaller.
    assert_eq!(v.capacity(), cap_after_first_fill);
    assert_eq!(v.as_mut_ptr(), ptr_after_first_fill);
    assert_eq!(&v[..], &[1.0, 2.0]);
}

#[test]
fn aligned_vec_truncate_shrinks_without_reallocating() {
    let mut v = AlignedVec::<f32>::zeroed(8);
    v.as_mut_slice()
        .copy_from_slice(&[1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0]);
    let cap_before = v.capacity();
    v.truncate(3);
    assert_eq!(v.len(), 3);
    assert_eq!(v.capacity(), cap_before);
    assert_eq!(&v[..], &[1.0_f32, 2.0, 3.0]);
}

#[test]
fn aligned_vec_extend_from_iter_appends_mapped_values() {
    let mut v = AlignedVec::<f32>::with_capacity(4);
    v.extend_from_iter((1..=4_u32).map(|x| x as f32 * 0.5));
    assert_eq!(&v[..], &[0.5, 1.0, 1.5, 2.0]);
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

// -- Phase 16: New collection ops ---------------------------------------------

#[test]
fn aligned_vec_pop_returns_last_element() {
    let mut v = AlignedVec::<u32>::from_slice(&[10, 20, 30]);
    assert_eq!(v.pop(), Some(30));
    assert_eq!(v.pop(), Some(20));
    assert_eq!(v.pop(), Some(10));
    assert_eq!(v.pop(), None);
}

#[test]
fn aligned_vec_swap_remove_unordered() {
    let mut v = AlignedVec::<i32>::from_slice(&[1, 2, 3, 4, 5]);
    let r = v.swap_remove(1); // removes 2, swaps 5 in
    assert_eq!(r, 2);
    assert_eq!(v.len(), 4);
    assert!(!v.as_slice().contains(&2));
    assert!(v.as_slice().contains(&5));
}

#[test]
fn aligned_vec_remove_preserves_order() {
    let mut v = AlignedVec::<i32>::from_slice(&[10, 20, 30, 40]);
    assert_eq!(v.remove(1), 20);
    assert_eq!(v.as_slice(), &[10, 30, 40]);
}

#[test]
fn aligned_vec_insert_shifts_right() {
    let mut v = AlignedVec::<i32>::from_slice(&[1, 3]);
    v.insert(1, 2);
    assert_eq!(v.as_slice(), &[1, 2, 3]);
}

#[test]
fn aligned_vec_retain_filters_in_place() {
    let mut v = AlignedVec::<i32>::from_slice(&[1, 2, 3, 4, 5, 6]);
    v.retain(|&x| x % 2 == 0);
    assert_eq!(v.as_slice(), &[2, 4, 6]);
}

#[test]
fn aligned_vec_dedup_consecutive() {
    let mut v = AlignedVec::<i32>::from_slice(&[1, 1, 2, 3, 3, 3, 2]);
    v.dedup();
    assert_eq!(v.as_slice(), &[1, 2, 3, 2]);
}

#[test]
fn aligned_vec_sort_and_dedup() {
    let mut v = AlignedVec::<i32>::from_slice(&[3, 1, 2, 1, 3, 2]);
    v.sort_unstable();
    v.dedup();
    assert_eq!(v.as_slice(), &[1, 2, 3]);
}

#[test]
fn aligned_vec_append_drains_source() {
    let mut a = AlignedVec::<u32>::from_slice(&[1, 2, 3]);
    let mut b = AlignedVec::<u32>::from_slice(&[4, 5, 6]);
    a.append(&mut b);
    assert_eq!(a.as_slice(), &[1, 2, 3, 4, 5, 6]);
    assert!(b.is_empty());
}

#[test]
fn aligned_vec_split_off() {
    let mut v = AlignedVec::<i32>::from_slice(&[1, 2, 3, 4, 5]);
    let tail = v.split_off(2);
    assert_eq!(v.as_slice(), &[1, 2]);
    assert_eq!(tail.as_slice(), &[3, 4, 5]);
}

#[test]
fn aligned_vec_shrink_to_fit() {
    let mut v = AlignedVec::<u32>::with_capacity(256);
    v.push(1);
    v.shrink_to_fit();
    assert_eq!(v.len(), 1);
    assert!(v.capacity() < 256);
}

#[test]
fn aligned_vec_drain_yields_and_collapses() {
    let mut v = AlignedVec::<i32>::from_slice(&[10, 20, 30, 40, 50]);
    let drained: std::vec::Vec<i32> = v.drain(1, 3).collect();
    assert_eq!(drained, &[20, 30]);
    assert_eq!(v.as_slice(), &[10, 40, 50]);
}

#[test]
fn aligned_vec_from_fn() {
    let v = AlignedVec::<u32>::from_fn(5, |i| (i as u32 + 1) * 10);
    assert_eq!(v.as_slice(), &[10, 20, 30, 40, 50]);
}

#[test]
fn aligned_vec_from_iterator() {
    let v: AlignedVec<u32> = (0u32..5).collect();
    assert_eq!(v.as_slice(), &[0, 1, 2, 3, 4]);
}

#[test]
fn aligned_vec_extend_and_extend_by_ref() {
    let mut v = AlignedVec::<u32>::from_slice(&[0, 1, 2]);
    v.extend(3u32..6);
    assert_eq!(v.as_slice(), &[0, 1, 2, 3, 4, 5]);
    let extra = [10u32, 20];
    v.extend(extra.iter());
    assert_eq!(&v.as_slice()[6..], &[10, 20]);
}

#[test]
fn aligned_vec_double_ended_iter() {
    let v = AlignedVec::<i32>::from_slice(&[1, 2, 3, 4]);
    let rev: std::vec::Vec<i32> = v.into_iter().rev().collect();
    assert_eq!(rev, &[4, 3, 2, 1]);
}

#[test]
fn aligned_vec_as_ref_and_borrow() {
    let v = AlignedVec::<u8>::from_slice(&[1, 2, 3]);
    let r: &[u8] = v.as_ref();
    assert_eq!(r, &[1, 2, 3]);
    use core::borrow::Borrow;
    let b: &[u8] = v.borrow();
    assert_eq!(b, &[1, 2, 3]);
}

#[test]
fn aligned_vec_spare_capacity_and_set_len() {
    let mut v = AlignedVec::<u32>::with_capacity(8);
    v.push(0);
    let spare = v.spare_capacity_mut();
    unsafe {
        let slice = &mut *spare;
        slice[0] = 1;
        slice[1] = 2;
        v.set_len_unchecked(3);
    }
    assert_eq!(v.as_slice(), &[0, 1, 2]);
}

#[test]
fn scratch_pool_prewarm_grows_primary() {
    let pool = ScratchPool::<f64>::new();
    assert_eq!(pool.capacity(), 0);
    pool.prewarm(128);
    assert!(pool.capacity() >= 128);
}

#[test]
fn scratch_pool_shrink_all_slots_releases_memory() {
    let pool = ScratchPool::<u32>::new();
    pool.with_scratch(64, |_| {});
    assert!(pool.capacity() >= 64);
    pool.shrink_all_slots();
    assert_eq!(pool.capacity(), 0);
}
#[test]
fn scratch_pool_with_scratch_uninit_initialises_before_read() {
    let pool = ScratchPool::<u32>::new();
    let result = unsafe {
        pool.with_scratch_uninit(4, |raw| {
            let slice = &mut *raw;
            for (i, elem) in slice.iter_mut().enumerate() {
                core::ptr::write(elem, i as u32 * 10);
            }
            (*raw)[3]
        })
    };
    assert_eq!(result, 30);
}

#[test]
fn aligned_vec_shrink_reduces_capacity_and_clamps_len() {
    let mut v = AlignedVec::<f64>::with_capacity(256);
    v.ensure_len(256);
    v.truncate(16);
    v.shrink_to(32);
    assert_eq!(v.capacity(), 32);
    assert_eq!(v.len(), 16, "initialized prefix survives a shrink");
    // Growth past the shrunk capacity works and re-zeros the new range.
    v.ensure_len(64);
    assert_eq!(v.len(), 64);
    assert!(
        v.as_slice()[16..].iter().all(|&x| x == 0.0),
        "newly grown range is zeroed"
    );
}

#[test]
fn aligned_vec_shrink_to_zero_frees_and_never_over_shrinks() {
    let mut v = AlignedVec::<f64>::zeroed(128);
    v.shrink_to(0);
    assert_eq!((v.capacity(), v.len()), (0, 0));
    v.shrink_to(0);
    assert_eq!(v.capacity(), 0, "shrinking an empty buffer is a no-op");
    let mut grown = AlignedVec::<u8>::zeroed(64);
    grown.shrink_to(128);
    assert_eq!(
        grown.capacity(),
        64,
        "shrinking to a larger size is a no-op"
    );
}

/// The retention fix must not trade memory savings for excessive reallocation
/// and copy traffic on the warm-up path.
#[test]
fn aligned_vec_growth_stays_geometric() {
    const TARGET_LEN: usize = 1 << 16;
    let mut v = AlignedVec::<f64>::dangling();
    let mut previous_capacity = 0;
    let mut growth_events = 0;
    for len in 1..=TARGET_LEN {
        v.ensure_len(len);
        let capacity = v.capacity();
        if capacity != previous_capacity {
            if previous_capacity != 0 {
                assert!(
                    capacity >= previous_capacity * 2,
                    "growth must at least double capacity: {previous_capacity} -> {capacity}"
                );
            }
            growth_events += 1;
            previous_capacity = capacity;
        }
    }
    assert!(
        growth_events <= 17,
        "2^16 extensions should need at most 17 geometric allocations, got {growth_events}"
    );
}

#[test]
fn pool_release_reclaims_above_provision() {
    let pool = ScratchPool::<f64>::new();
    let mut max_seen = 0usize;
    for n in [128usize, 16_384, 512] {
        pool.with_scratch_bounded(n, |s| {
            assert_eq!(s.len(), n);
            s[0] = 1.0;
            max_seen = max_seen.max(n);
        });
    }
    let caps = pool.release();
    assert_eq!(caps[0], 16_384, "kept exactly the working set");
    // The capacity mirror (the reentrant accessor) is unchanged by a warm pass:
    // growth would update it, so equality here is the no-new-allocation proof.
    let before = pool.capacity();
    pool.with_scratch_bounded(16_384, |s| {
        s[0] += 1.0;
    });
    assert_eq!(
        pool.capacity(),
        before,
        "post-release warm pass must not grow (allocate)"
    );
}

#[test]
fn pool_release_without_provision_frees_everything() {
    let pool = ScratchPool::<f64>::new();
    pool.with_scratch(1_024, |s| {
        s[0] = 1.0;
    });
    let caps = pool.release();
    assert_eq!(caps[0], 0, "unprovisioned slot reclaims entirely");
    assert_eq!(pool.capacity(), 0, "capacity mirror follows the reclaim");
}

#[test]
fn pool_release_skips_busy_slots_and_reset_re_enables() {
    let pool = ScratchPool::<f64>::new();
    pool.with_scratch_bounded(4_096, |_| {
        pool.with_scratch_bounded(2_048, |_| {
            // Depths 0 and 1 are busy; only 2.. are idle (and unprovisioned).
            let caps = pool.release();
            assert_eq!(caps[0], 4_096, "busy slot reports its provision");
            assert_eq!(caps[1], 2_048);
            assert_eq!(caps[2], 0);
        });
    });
    let caps = pool.release();
    assert_eq!(caps[0], 4_096);
    // Depth 1 holds 2_048 exactly (its provision), so nothing is reclaimed and
    // the reported capacity is the held one.
    assert_eq!(caps[1], 2_048);
    pool.reset();
    let caps = pool.release();
    assert_eq!(caps[0], 0, "reset makes release reclaim everything");
    assert_eq!(caps[1], 0);
}

#[test]
fn pool_release_converges_across_growth_cycles() {
    // Three grow/shrink cycles; the pool must end at the high-water working
    // set and stay allocation-free on the warm path throughout.
    let pool = ScratchPool::<f64>::new();
    for &n in &[512usize, 65_536, 8_192] {
        pool.with_scratch_bounded(n, |_| {});
        pool.release();
    }
    // Provisions are high-water marks: the retained slot covers the largest
    // request of the whole cycle, not the latest one.
    assert_eq!(pool.release()[0], 65_536);
    // A working-set changeover (reset, run the new set, release) reclaims the
    // rest — the steady state tracks the current workload, not the historical
    // peak, once the consumer signals the changeover.
    pool.reset();
    pool.with_scratch_bounded(512, |_| {});
    assert_eq!(pool.release()[0], 512);
    let before = pool.capacity();
    pool.with_scratch_bounded(512, |s| {
        s[511] = 1.0;
    });
    assert_eq!(pool.capacity(), before, "warm pass stays allocation-free");
}

#[test]
fn total_capacity_counts_slots_grown_by_nested_borrows() {
    let pool: ScratchPool<f64> = ScratchPool::new();
    let element = size_of::<f64>();

    // Slot 0 from the outer borrow, slot 1 from the nested one. Reading the
    // total from *inside* the nested borrow is the case a slot-0-only answer
    // got wrong: both slots are grown, and one of them is held exclusively.
    pool.with_scratch(1024, |outer| {
        outer[0] = 1.0;
        pool.with_scratch(512, |inner| {
            inner[0] = 2.0;
            let total = pool.total_capacity_bytes();
            assert!(
                total >= (1024 + 512) * element,
                "total_capacity_bytes reported {total} bytes during a nested                  borrow, which cannot cover slot 0 (>=1024) plus slot 1 (>=512)                  at {element} bytes per element"
            );
            assert!(
                total > pool.capacity() * element,
                "the total must exceed slot 0 alone once a nested borrow has                  grown a second slot"
            );
        });
    });

    // The same total holds once every borrow has ended.
    let quiescent = pool.total_capacity_bytes();
    assert!(quiescent >= (1024 + 512) * element);

    let freed: usize = pool.release().iter().sum();
    assert_eq!(freed, 0, "release leaves every slot at zero capacity");
    assert_eq!(pool.total_capacity_bytes(), 0);
}

#[test]
fn uninit_callback_panic_leaves_no_uninitialized_length_behind() {
    // Spare capacity with a shorter initialized length is the case that makes
    // this reachable: `with_scratch_uninit` skips `ensure_len`, so `[len, n)`
    // stays uninitialized while the callback runs.
    let pool: ScratchPool<u64> = ScratchPool::with_slot_capacity(1024);
    assert!(pool.capacity() >= 1024);

    let panicked = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        // SAFETY: the callback is required to initialize the range, and it
        // deliberately does not — it unwinds first, which is the case under
        // test. Nothing reads the buffer through this call.
        unsafe {
            pool.with_scratch_uninit(512, |_raw| {
                panic!("callback unwinds before initializing anything");
            })
        }
    }));
    assert!(panicked.is_err(), "the callback must have unwound");

    // The safe path must not inherit a length covering memory the unwound
    // callback never wrote. Under Miri this read is the assertion: an
    // uninitialized element here is undefined behaviour, not a wrong value.
    pool.with_scratch(512, |scratch| {
        assert_eq!(scratch.len(), 512);
        let observed = scratch.iter().fold(0u64, |acc, value| acc ^ *value);
        assert_eq!(observed, 0, "reused scratch must be zeroed, not stale");
    });
}
// ---- Phase 17: resize_with / fill_with -------------------------------------

#[test]
fn aligned_vec_resize_with_grows_using_closure() {
    let mut v = AlignedVec::<u32>::from_slice(&[1, 2]);
    let mut counter = 10u32;
    v.resize_with(5, || {
        counter += 1;
        counter
    });
    assert_eq!(v.len(), 5);
    assert_eq!(&v.as_slice()[..2], &[1, 2]);
    // Elements 2..5 produced by closure
    assert!(v.as_slice()[2] > 10);
    assert!(v.as_slice()[4] > v.as_slice()[2]);
}

#[test]
fn aligned_vec_resize_with_shrinks_like_truncate() {
    let mut v = AlignedVec::<i32>::from_slice(&[1, 2, 3, 4, 5]);
    v.resize_with(3, || 99);
    assert_eq!(v.as_slice(), &[1, 2, 3]);
}

#[test]
fn aligned_vec_fill_with_overwrites_all_elements() {
    let mut v = AlignedVec::<u32>::from_slice(&[0, 0, 0, 0]);
    let mut idx = 0u32;
    v.fill_with(|| {
        idx += 1;
        idx
    });
    assert_eq!(v.as_slice(), &[1, 2, 3, 4]);
}
// ---- Phase 18b: concat / binary_search / contains / position ---------------

#[test]
fn aligned_vec_concat_merges_two_slices() {
    let v = AlignedVec::<u32>::concat(&[1, 2, 3], &[4, 5, 6]);
    assert_eq!(v.as_slice(), &[1, 2, 3, 4, 5, 6]);
}

#[test]
fn aligned_vec_concat_with_empty() {
    let v = AlignedVec::<u8>::concat(&[10, 20], &[]);
    assert_eq!(v.as_slice(), &[10, 20]);
    let v2 = AlignedVec::<u8>::concat(&[], &[30, 40]);
    assert_eq!(v2.as_slice(), &[30, 40]);
}

#[test]
fn aligned_vec_binary_search_finds_element() {
    let v = AlignedVec::<i32>::from_slice(&[1, 3, 5, 7, 9]);
    assert_eq!(v.binary_search(&5), Ok(2));
    assert!(v.binary_search(&4).is_err());
}

#[test]
fn aligned_vec_contains_and_position() {
    let v = AlignedVec::<u32>::from_slice(&[10, 20, 30, 20]);
    assert!(v.contains(&20));
    assert!(!v.contains(&99));
    assert_eq!(v.position(&20), Some(1));
    assert_eq!(v.position(&99), None);
}

#[test]
fn aligned_vec_sort_unstable_inplace() {
    let mut v = AlignedVec::<i32>::from_slice(&[3, 1, 4, 1, 5, 9, 2, 6]);
    v.sort_unstable_inplace();
    assert_eq!(v.as_slice(), &[1, 1, 2, 3, 4, 5, 6, 9]);
}
// ---- Phase 20: zero_fill / copy_from_slice / as_ptr_range -----------------

#[test]
fn aligned_vec_zero_fill_resets_all_elements() {
    let mut v = AlignedVec::<u32>::from_slice(&[1, 2, 3, 4]);
    v.zero_fill();
    assert_eq!(v.as_slice(), &[0, 0, 0, 0]);
}

#[test]
fn aligned_vec_zero_fill_empty_is_noop() {
    let mut v = AlignedVec::<u8>::dangling();
    v.zero_fill(); // must not panic
    assert!(v.is_empty());
}

#[test]
fn aligned_vec_copy_from_slice() {
    let mut v = AlignedVec::<i32>::zeroed(4);
    v.copy_from_slice(&[10, 20, 30, 40]);
    assert_eq!(v.as_slice(), &[10, 20, 30, 40]);
}

#[test]
fn aligned_vec_as_ptr_range() {
    let v = AlignedVec::<u32>::from_slice(&[1, 2, 3]);
    let range = v.as_ptr_range();
    assert_eq!(range.end as usize - range.start as usize, 3 * core::mem::size_of::<u32>());
    assert_eq!(range.start, v.as_ptr());
}

#[test]
fn aligned_vec_as_mut_ptr_range() {
    let mut v = AlignedVec::<u32>::from_slice(&[1, 2, 3]);
    let range = v.as_mut_ptr_range();
    assert_eq!(range.end as usize - range.start as usize, 3 * core::mem::size_of::<u32>());
}