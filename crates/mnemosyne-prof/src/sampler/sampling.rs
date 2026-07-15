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

    // SAFETY: `get_profiler_state()` returns this thread's own thread-local
    // `ThreadState`; the `&mut` is exclusive (thread-local) and this runs inside
    // the `enter_hook`/`exit_hook` re-entrancy guard, so no nested `&mut` to the
    // same state can be live.
    #[cfg(not(nightly_tls_active))]
    unsafe {
        let state = &mut *crate::get_profiler_state();
        maybe_record_sample(ptr, size, leak_active, debit, &mut state.bytes_until_sample);
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
