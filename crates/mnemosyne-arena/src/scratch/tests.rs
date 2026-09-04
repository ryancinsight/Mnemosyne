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

#[test]
fn release_frees_every_slot_and_reports_the_bytes() {
    let pool: ScratchPool<f64> = ScratchPool::new();
    pool.with_scratch(1024, |scratch| scratch[0] = 1.0);
    let grown = pool.capacity();
    assert!(
        grown >= 1024,
        "the slot must have grown to serve the request"
    );

    let freed = pool.release().expect("no borrow is live");
    assert!(
        freed >= grown * size_of::<f64>(),
        "released {freed} bytes against a slot of {grown} f64 elements"
    );
    assert_eq!(pool.capacity(), 0, "the slot allocation is gone");

    // Still usable: the slot re-grows on demand.
    pool.with_scratch(16, |scratch| assert_eq!(scratch.len(), 16));
    assert!(pool.capacity() >= 16);
}

#[test]
fn release_refuses_while_a_borrow_is_live_and_frees_nothing() {
    let pool: ScratchPool<f64> = ScratchPool::new();
    pool.with_scratch(512, |scratch| scratch[0] = 1.0);
    let before = pool.capacity();
    assert!(before >= 512);

    pool.with_scratch(512, |scratch| {
        scratch[0] = 2.0;
        // Freeing here would invalidate `scratch`, which is still held.
        assert!(
            pool.release().is_none(),
            "release must refuse while its own slot is borrowed"
        );
        // The guard has to be load-bearing, not decorative: the slice must
        // still be usable after the refused call.
        assert_eq!(scratch[0], 2.0);
    });

    assert_eq!(
        pool.capacity(),
        before,
        "a refused release must free nothing"
    );
    assert!(pool.release().is_some(), "and succeed once the borrow ends");
}

#[test]
fn bank_release_is_all_or_nothing_across_pools() {
    let bank: ScratchBank<f64, 2> = ScratchBank::new();
    bank.with_scratch::<0, _>(512, |scratch| scratch[0] = 1.0);
    bank.with_scratch::<1, _>(256, |scratch| scratch[0] = 2.0);
    assert!(bank.capacity::<0>() >= 512 && bank.capacity::<1>() >= 256);

    bank.with_scratch::<0, _>(512, |_| {
        assert!(
            bank.release().is_none(),
            "one live borrow must block the whole bank"
        );
    });
    assert!(
        bank.capacity::<1>() >= 256,
        "the untouched pool keeps its buffer when the bank refuses"
    );

    let freed = bank.release().expect("quiescent");
    assert!(freed >= (512 + 256) * size_of::<f64>());
    assert_eq!(bank.capacity::<0>(), 0);
    assert_eq!(bank.capacity::<1>(), 0);
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