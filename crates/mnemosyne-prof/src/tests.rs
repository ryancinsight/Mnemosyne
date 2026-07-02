use core::sync::atomic::Ordering;

#[test]
fn sample_metadata_stores_fixed_stack_id() {
    assert_eq!(
        core::mem::size_of::<crate::sampler::Sample>(),
        core::mem::size_of::<usize>() * 2,
        "sample metadata must store size plus fixed-width stack identity only"
    );
}

/// Regression test: a block sampled under the leak detector and freed after
/// `disable_leak_detector()` must still be evicted — otherwise a later
/// `dump_leaks` falsely reports it and `ACTIVE_SAMPLES_COUNT` never drains.
#[test]
fn free_after_leak_detector_disable_drains_resident_samples() {
    crate::reset_profiler_for_testing();
    crate::enable_leak_detector();

    // `on_alloc`/`on_free` treat the pointer as an opaque map key and never
    // dereference it, so a synthetic address keeps this test independent of
    // the real allocator.
    let ptr = 0x0006_4000_usize as *mut u8;
    crate::on_alloc(ptr, 256);
    assert_eq!(
        crate::ACTIVE_SAMPLES_COUNT.load(Ordering::Relaxed),
        1,
        "leak detector must record the allocation as a resident sample"
    );

    crate::disable_leak_detector();
    crate::on_free(ptr, 256);
    assert_eq!(
        crate::ACTIVE_SAMPLES_COUNT.load(Ordering::Relaxed),
        0,
        "a free after disabling the leak detector must drain the resident sample"
    );

    let path = std::env::temp_dir().join(format!(
        "mnemosyne_prof_drain_test_{}.txt",
        std::process::id()
    ));
    let path_str = path
        .to_str()
        .expect("temporary leak-report path must be valid UTF-8");
    let leaks = crate::dump_leaks(path_str).expect("dump_leaks must succeed on a writable path");
    assert_eq!(
        leaks, 0,
        "a block freed after leak-detector disable must not be reported as a leak"
    );
    let _ = std::fs::remove_file(&path);

    crate::reset_profiler_for_testing();
}

/// Regression test: the leak detector must track every allocation even when
/// this thread carries a large residual `bytes_until_sample` budget from a
/// prior profiling session. An inverted flag in `on_alloc` previously let the
/// stale budget's fast skip hide allocations from leak tracking.
#[test]
fn leak_detector_tracks_allocations_despite_stale_sampling_budget() {
    crate::reset_profiler_for_testing();

    // Seed this thread's sampling budget: one sampled allocation under a 1 GiB
    // Poisson interval leaves `bytes_until_sample` ≈ 1 GiB (the probability of
    // an exponential draw below the 64-byte probe is ~6e-8, i.e. negligible).
    crate::enable_profiling(1 << 30);
    let warm = 0x0007_0000_usize as *mut u8;
    crate::on_alloc(warm, 64);
    crate::on_free(warm, 64);
    crate::disable_profiling();

    crate::enable_leak_detector();
    let ptr = 0x0007_4000_usize as *mut u8;
    crate::on_alloc(ptr, 64);
    assert_eq!(
        crate::ACTIVE_SAMPLES_COUNT.load(Ordering::Relaxed),
        1,
        "leak detector must track an allocation despite a stale sampling budget"
    );
    crate::on_free(ptr, 64);
    assert_eq!(crate::ACTIVE_SAMPLES_COUNT.load(Ordering::Relaxed), 0);

    crate::reset_profiler_for_testing();
}
