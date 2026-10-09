use core::sync::atomic::Ordering;

use crate::SAMPLE_INTERVAL;

use super::capture::{capture_stack, next_sample_interval};
use super::stack_interner::{release_stack, reset_stack_interner_state};
use super::store::{Sample, insert_sample, remove_sample, reset_active_samples};

/// Reset the sampler state (active samples).
pub(crate) fn reset_sampler_state() {
    reset_active_samples();
    reset_stack_interner_state();
}

pub(crate) fn sample_alloc_inner(ptr: *mut u8, size: usize, leak_active: bool) {
    let debit = crate::sample_debit(size);

    #[cfg(nightly_tls_active)]
    {
        let mut val = crate::get_bytes_until_sample();
        maybe_record_sample(ptr, size, leak_active, debit, &mut val);
        crate::set_bytes_until_sample(val);
    }

    // The counter is copied out and written back, as the nightly path above
    // does, rather than lent as `&mut` into the thread state: stack capture
    // allocates, and the re-entrant `should_skip_alloc_fast_path` reborrows the
    // whole state to read `in_hook`, which a live `&mut` into it forbids.
    #[cfg(not(nightly_tls_active))]
    {
        let state = crate::get_profiler_state();
        // SAFETY: `get_profiler_state()` returns this thread's own live
        // thread-local `ThreadState`, and no reference into it is held here.
        let mut val = unsafe { (*state).bytes_until_sample };
        maybe_record_sample(ptr, size, leak_active, debit, &mut val);
        // SAFETY: as above; the nested allocations stack capture made have
        // returned, and none of them reached this counter (the `in_hook` guard
        // is raised for the whole sample).
        unsafe { (*state).bytes_until_sample = val };
    }
}

fn maybe_record_sample(
    ptr: *mut u8,
    size: usize,
    leak_active: bool,
    debit: isize,
    bytes_until_sample: &mut isize,
) {
    if leak_active || *bytes_until_sample <= debit {
        if !leak_active {
            let mean = SAMPLE_INTERVAL.load(Ordering::Relaxed);
            *bytes_until_sample = next_sample_interval(mean) as isize;
        }

        let stack = capture_stack();
        let replaced = insert_sample(ptr as usize, Sample { size, stack });
        if let Some(replaced) = replaced {
            release_stack(replaced.stack);
        }
    }

    if !leak_active {
        *bytes_until_sample = (*bytes_until_sample).saturating_sub(debit);
    }
}

pub(crate) fn sample_free_inner(ptr: *mut u8) {
    let removed = remove_sample(ptr as usize);
    if let Some(sample) = removed {
        release_stack(sample.stack);
    }
}

#[cfg(test)]
mod tests {
    use super::sample_alloc_inner;

    fn bytes_until_sample() -> isize {
        #[cfg(nightly_tls_active)]
        {
            crate::get_bytes_until_sample()
        }
        #[cfg(not(nightly_tls_active))]
        // SAFETY: this thread's own live `ThreadState`; no reference into it
        // is held across the read.
        unsafe {
            (*crate::get_profiler_state()).bytes_until_sample
        }
    }

    fn set_bytes_until_sample(value: isize) {
        #[cfg(nightly_tls_active)]
        crate::set_bytes_until_sample(value);
        #[cfg(not(nightly_tls_active))]
        // SAFETY: as in `bytes_until_sample`.
        unsafe {
            (*crate::get_profiler_state()).bytes_until_sample = value;
        }
    }

    /// An allocation smaller than the remaining budget records no sample and
    /// debits exactly its size from this thread's counter.
    #[test]
    fn unsampled_allocation_debits_its_size_from_the_thread_counter() {
        const BUDGET: isize = 100_000;
        const SIZE: usize = 4_096;
        set_bytes_until_sample(BUDGET);
        // The pointer is an opaque key: no sample is recorded below the
        // budget, so it is never dereferenced.
        sample_alloc_inner(0x0007_1000_usize as *mut u8, SIZE, false);
        assert_eq!(bytes_until_sample(), BUDGET - 4_096);
        sample_alloc_inner(0x0007_2000_usize as *mut u8, SIZE, false);
        assert_eq!(bytes_until_sample(), BUDGET - 8_192);
    }
}
